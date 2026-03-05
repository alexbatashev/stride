use crate::utils::TransportHandle;

use bytes::Bytes;
use http_body_util::Full;
use hyper::Request;
use crate::{
    API, Completion, CompletionChoice, CompletionRequest, Delta, EmbeddingData, EmbeddingResponse,
    Error, Message, ModelDesc, StreamResponseChunk, Usage,
};

use async_stream::stream;
use futures::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use std::pin::Pin;
use uuid::Uuid;

#[derive(Clone)]
pub struct Ollama {
    base_url: String,
    _transport: TransportHandle,
}

impl std::fmt::Debug for Ollama {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ollama")
            .field("base_url", &self.base_url)
            .finish()
    }
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
    pub fn new(base_url: &str, transport: TransportHandle) -> API {
        API::Ollama(Ollama {
            base_url: base_url.to_string(),
            _transport: transport,
        })
    }

    pub async fn list_models(&self, _token: &str) -> Result<Vec<ModelDesc>, Error> {
        let req = Request::builder()
            .method("GET")
            .uri(&format!("{}/api/tags", self.base_url))
            .body(Full::new(Bytes::new()))
            .map_err(|e| Error::InvalidRequest(e.to_string()))?;
        let (status, res_body) = crate::net::send_request(req).await?;
        if !(200..300).contains(&status) {
            return Err(Error::ServerError(status));
        }

        let model_list: Models =
            serde_json::from_slice(&res_body).map_err(|e| Error::ParsingError(format!("{:?}", e)))?;

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
        #[derive(Serialize, Debug, Clone)]
        struct Body<'a> {
            model: &'a str,
        }

        let body = serde_json::to_vec(&Body { model }).map_err(|_| Error::Unknown)?;

        let req = Request::builder()
            .method("POST")
            .uri(&format!("{}/api/show", self.base_url))
            .header("Content-Type", "application/json")
            .body(Full::new(Bytes::from(body)))
            .map_err(|e| Error::InvalidRequest(e.to_string()))?;
        let (status, _) = crate::net::send_request(req).await?;
        if !(200..300).contains(&status) {
            return Err(Error::ServerError(status));
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
        #[derive(Debug, Clone, Serialize)]
        struct RequestData<'a> {
            input: &'a str,
            model: &'a str,
        }

        let body = serde_json::to_vec(&RequestData { input, model }).map_err(|_| Error::Unknown)?;

        let req = Request::builder()
            .method("POST")
            .uri(&format!("{}/api/embed", self.base_url))
            .header("Content-Type", "application/json")
            .body(Full::new(Bytes::from(body)))
            .map_err(|e| Error::InvalidRequest(e.to_string()))?;
        let (status, res_body) = crate::net::send_request(req).await?;
        if !(200..300).contains(&status) {
            return Err(Error::ServerError(status));
        }

        #[derive(Deserialize)]
        struct OllamaResponse {
            model: String,
            embeddings: Vec<Vec<f32>>,
            prompt_eval_count: u32,
        }

        let mut embeddings: OllamaResponse = serde_json::from_slice(&res_body)
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
        let body = serde_json::to_vec(&ChatRequest {
            model: &request.model,
            stream: false,
            messages: request.messages.clone(),
            think: true,
        })
        .map_err(|_| Error::Unknown)?;

        let req = Request::builder()
            .method("POST")
            .uri(&format!("{}/api/chat", self.base_url))
            .header("Content-Type", "application/json")
            .body(Full::new(Bytes::from(body)))
            .map_err(|e| Error::InvalidRequest(e.to_string()))?;
        let (status, res_body) = crate::net::send_request(req).await?;
        if !(200..300).contains(&status) {
            return Err(Error::ServerError(status));
        }

        let message: MessageResponse = serde_json::from_slice(&res_body).map_err(|e| {
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
        let body = match serde_json::to_vec(&ChatRequest {
            model: &request.model,
            stream: true,
            messages: request.messages.clone(),
            think: true,
        }) {
            Ok(b) => b,
            Err(_) => {
                return Box::pin(futures::stream::once(async { Err(Error::Unknown) }));
            }
        };

        let req = match Request::builder()
            .method("POST")
            .uri(&format!("{}/api/chat", self.base_url))
            .header("Content-Type", "application/json")
            .body(Full::new(Bytes::from(body)))
            .map_err(|e| Error::InvalidRequest(e.to_string()))
        {
            Ok(req) => req,
            Err(err) => return Box::pin(futures::stream::once(async move { Err(err) })),
        };

        let s = stream! {
            let mut upstream = crate::net::stream_request(req, |data| Ok::<Vec<u8>, Error>(data.to_vec())).await;
            let mut buf = Vec::new();

            while let Some(item) = upstream.next().await {
                let data = match item {
                    Ok(d) => d,
                    Err(e) => {
                        yield Err(e);
                        return;
                    }
                };
                buf.extend_from_slice(&data);

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
                                        finish_reason: data.done_reason.clone(),
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
