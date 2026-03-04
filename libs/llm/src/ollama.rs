use crate::completion_request::CompletionRequest;
use crate::utils::{SharedExecutor, get_http_client};
use crate::{
    API, Completion, CompletionChoice, Delta, EmbeddingData, EmbeddingResponse, Error, Message,
    ModelDesc, StreamResponseChunk, Usage,
};

use async_stream::stream;
use bytes::Bytes;
use futures::Stream;
use http_body_util::{BodyExt, Full};
use hyper::header::CONTENT_TYPE;
use hyper::{Method, Request};
use serde::{Deserialize, Serialize};
use std::pin::Pin;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct Ollama {
    base_url: String,
    executor: SharedExecutor,
}

#[derive(Debug, Clone, Deserialize)]
struct ModelEntry {
    model: String,
}

#[derive(Debug, Clone, Deserialize)]
struct Models {
    models: Vec<ModelEntry>,
}

#[derive(Debug, Clone, Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    stream: bool,
    messages: Vec<Message>,
    think: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct MessageResponse {
    model: String,
    message: Message,
    done: bool,
    done_reason: Option<String>,
    prompt_eval_count: Option<u32>,
    eval_count: Option<u32>,
}

impl Ollama {
    pub fn new(base_url: &str, executor: SharedExecutor) -> API {
        API::Ollama(Ollama {
            base_url: base_url.to_string(),
            executor,
        })
    }

    pub async fn list_models(&self, _token: &str) -> Result<Vec<ModelDesc>, Error> {
        let url = format!("{}/api/tags", self.base_url);

        let client = get_http_client(self.executor.clone())?;

        let req = Request::builder()
            .method(Method::GET)
            .uri(url)
            .body(Full::new(Bytes::new()))
            .map_err(|_| Error::Unknown)?;

        let res = client
            .request(req)
            .await
            .map_err(|e| Error::RequestError(format!("{:?}", e)))?;

        let status = res.status();

        let body_bytes = res
            .into_body()
            .collect()
            .await
            .map_err(|e| Error::ParsingError(format!("{:?}", e)))?
            .to_bytes();

        if !status.is_success() {
            return Err(Error::ServerError(status.as_u16()));
        }

        let model_list: Models = serde_json::from_slice(&body_bytes)
            .map_err(|e| Error::ParsingError(format!("{:?}", e)))?;

        Ok(model_list
            .models
            .into_iter()
            .map(|me| ModelDesc {
                id: me.model,
                object: "model".to_string(),
                created: None,
                owned_by: None,
            })
            .collect())
    }

    pub async fn get_model(&self, _token: &str, model: &str) -> Result<ModelDesc, Error> {
        let url = format!("{}/api/show", self.base_url);

        let client = get_http_client(self.executor.clone())?;

        #[derive(Serialize, Debug, Clone)]
        struct Body<'a> {
            model: &'a str,
        }

        let body = serde_json::to_string(&Body { model }).map_err(|_| Error::Unknown)?;

        let req = Request::builder()
            .method(Method::POST)
            .uri(url)
            .header(CONTENT_TYPE, "application/json")
            .body(Full::new(Bytes::from(body)))
            .map_err(|_| Error::Unknown)?;

        let res = client
            .request(req)
            .await
            .map_err(|e| Error::RequestError(format!("{:?}", e)))?;

        let status = res.status();

        res.into_body()
            .collect()
            .await
            .map_err(|e| Error::ParsingError(format!("{:?}", e)))?
            .to_bytes();

        if !status.is_success() {
            return Err(Error::ServerError(status.as_u16()));
        }

        Ok(ModelDesc {
            id: model.to_string(),
            object: "model".to_string(),
            created: None,
            owned_by: None,
        })
    }

    pub async fn get_embeddings(
        &self,
        _token: &str,
        input: &str,
        model: &str,
    ) -> Result<EmbeddingResponse, Error> {
        let url = format!("{}/api/embed", self.base_url);

        #[derive(Debug, Clone, Serialize)]
        struct RequestData<'a> {
            input: &'a str,
            model: &'a str,
        }

        let json =
            serde_json::to_string(&RequestData { input, model }).map_err(|_| Error::Unknown)?;

        let req = Request::builder()
            .method(Method::POST)
            .uri(url)
            .header(CONTENT_TYPE, "application/json")
            .body(Full::new(Bytes::from(json)))
            .map_err(|e| Error::RequestError(format!("{:?}", e)))?;

        eprintln!("CREATE REQUEST: {:?}", req);

        let client = get_http_client(self.executor.clone())?;

        let res = client.request(req).await.map_err(|_| Error::Unknown)?;
        eprintln!("REQUEST COMPLETE");
        let status = res.status();

        let body_bytes = res
            .into_body()
            .collect()
            .await
            .map_err(|e| Error::ParsingError(format!("{:?}", e)))?
            .to_bytes();

        if !status.is_success() {
            eprintln!("Bad request {}", String::from_utf8_lossy(&body_bytes));
            return Err(Error::ServerError(status.as_u16()));
        }

        #[derive(Deserialize)]
        struct OllamaResponse {
            model: String,
            embeddings: Vec<Vec<f32>>,
            prompt_eval_count: u32,
        }

        let mut embeddings: OllamaResponse = serde_json::from_slice(&body_bytes)
            .map_err(|e| Error::ParsingError(format!("{:?}", e)))?;

        let embeddings = EmbeddingResponse {
            object: "object".to_string(),
            model: embeddings.model,
            data: EmbeddingData {
                object: "list".to_string(),
                index: 0,
                embedding: std::mem::replace(&mut embeddings.embeddings[0], vec![]),
            },
            usage: Usage {
                prompt_tokens: embeddings.prompt_eval_count,
                completion_tokens: 0,
                total_tokens: embeddings.prompt_eval_count,
            },
        };

        Ok(embeddings)
    }

    pub async fn get_completion(
        &self,
        _token: &str,
        request: CompletionRequest,
    ) -> Result<Completion, Error> {
        let url = format!("{}/api/chat", self.base_url);

        let client = get_http_client(self.executor.clone())?;

        // FIXME: support options and tools
        let body = serde_json::to_string(&ChatRequest {
            model: &request.model,
            stream: false,
            messages: request.messages.clone(),
            think: true,
        })
        .map_err(|_| Error::Unknown)?;

        let req = Request::builder()
            .method(Method::POST)
            .uri(url)
            .header(CONTENT_TYPE, "application/json")
            .body(Full::new(Bytes::from(body)))
            .map_err(|_| Error::Unknown)?;

        let res = client
            .request(req)
            .await
            .map_err(|e| Error::RequestError(format!("{:?}", e)))?;

        let status = res.status();

        let body_bytes = res
            .into_body()
            .collect()
            .await
            .map_err(|e| Error::ParsingError(format!("{:?}", e)))?
            .to_bytes();

        if !status.is_success() {
            return Err(Error::ServerError(status.as_u16()));
        }

        let message: MessageResponse = serde_json::from_slice(&body_bytes).map_err(|e| {
            Error::ParsingError(format!("Failed to parse upstream response: {:?}", e))
        })?;

        let prompt_tokens = message.prompt_eval_count.unwrap_or_default();
        let completion_tokens = message.eval_count.unwrap_or_default();

        Ok(Completion {
            id: Uuid::now_v7().into(),
            created: 0,
            model: message.model,
            choices: vec![CompletionChoice {
                message: Some(message.message),
                text: None,
                index: 0,
                delta: None,
                logprobs: None,
                tool_calls: None,
                finish_reason: message.done_reason,
            }],
            usage: Usage {
                prompt_tokens,
                completion_tokens,
                total_tokens: prompt_tokens + completion_tokens,
            },
        })
    }

    pub fn stream_completion(
        &self,
        _token: &str,
        request: CompletionRequest,
    ) -> Pin<Box<dyn Stream<Item = Result<StreamResponseChunk, Error>> + Send + 'static>> {
        let base_url = self.base_url.clone();
        let executor = self.executor.clone();

        let s = stream! {
            let url = format!("{}/api/chat", base_url);

            let client = get_http_client(executor)?;

            // FIXME: support options and tools
            let body = serde_json::to_string(&ChatRequest {
                model: &request.model,
                stream: true,
                messages: request.messages.clone(),
                think: true,
            })
            .map_err(|_| Error::Unknown)?;

            let req = Request::builder()
                .method(Method::POST)
                .uri(url)
                .header(CONTENT_TYPE, "application/json")
                .body(Full::new(Bytes::from(body)))
                .map_err(|_| Error::Unknown)?;

            let mut res = client
                .request(req)
                .await
                .map_err(|e| Error::RequestError(format!("{:?}", e)))?;

            let status = res.status();

            if !status.is_success() {
                yield Err(Error::ServerError(res.status().as_u16()));
                return;
            }

            let mut buf = Vec::new();
            while let Some(frame) = res.frame().await {
                let frame = match frame {
                    Ok(f) if f.is_data() => f.into_data().unwrap(),
                    _ => continue
                };
                buf.extend_from_slice(&frame);

                while let Some(pos) = buf.windows(1).position(|w| w == b"\n") {
                    let event = buf.drain(..pos + 1).collect::<Vec<_>>();
                    let s = String::from_utf8_lossy(&event);
                    for line in s.lines() {
                        match serde_json::from_str::<MessageResponse>(line) {
                            Ok(data) => {
                                let chunk = StreamResponseChunk{
                                    id: Uuid::now_v7().into(),
                                    object: "completion".to_string(),
                                    created: 0,
                                    model: data.model,
                                    system_fingerprint: None,
                                    choices: vec![CompletionChoice {
                                        message: Some(data.message.clone()),
                                        text: None,
                                        index: 0,
                                        delta: Some(Delta {
                                            content: Some(data.message.content.clone()),
                                            thinking: data.message.thinking.clone(),
                                            tool_calls: None,
                                        }),
                                        logprobs: None,
                                        tool_calls: None,
                                        finish_reason: data.done_reason,
                                    }]
                                };
                                yield Ok(chunk);
                                if data.done {
                                    return;
                                }
                            },
                            Err(e) => { yield Err(Error::ParsingError(format!("{}", e))) },
                        }
                    }
                }
            }

        };
        Box::pin(s)
    }
}
