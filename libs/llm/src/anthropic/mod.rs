use std::pin::Pin;

use async_stream::stream;
use futures::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use uuid;

use crate::utils::{HttpRequest, TransportHandle};
use crate::{API, Completion, CompletionChoice, CompletionRequest, Error, Message, ModelDesc, StreamResponseChunk};

#[derive(Clone)]
pub struct Anthropic {
    base_url: String,
    transport: TransportHandle,
}

impl std::fmt::Debug for Anthropic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Anthropic")
            .field("base_url", &self.base_url)
            .finish()
    }
}

#[derive(Deserialize)]
struct AnthropicTextContent {
    r#type: String,
    text: String,
}

#[derive(Deserialize)]
struct AnthropicCompletionResponse {
    id: String,
    r#type: String,
    role: String,
    model: String,
    content: Vec<AnthropicTextContent>,
    stop_reason: Option<String>,
    stop_sequence: Option<String>,
    usage: AnthropicUsage,
}

#[derive(Deserialize)]
struct AnthropicUsage {
    input_tokens: u32,
    output_tokens: u32,
}

#[derive(Serialize)]
struct AnthropicCompletionRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    messages: &'a [Message],
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
}

impl Anthropic {
    pub fn new(base_url: &str, transport: TransportHandle) -> API {
        API::Anthropic(Anthropic {
            base_url: base_url.to_string(),
            transport,
        })
    }

    pub async fn list_models(&self, token: &str) -> Result<Vec<ModelDesc>, Error> {
        #[derive(Debug, Deserialize)]
        struct AnthropicModelList {
            models: Vec<ModelDesc>,
        }

        let req = HttpRequest {
            method: "GET".to_string(),
            url: format!("{}/v1/models", self.base_url.trim_end_matches('/')),
            headers: vec![
                ("x-api-key".to_string(), token.to_string()),
                ("anthropic-version".to_string(), "2023-06-01".to_string()),
            ],
            body: vec![],
        };

        let res = self.transport.0.request(req).await?;
        if !(200..300).contains(&res.status) {
            return Err(Error::ServerError(res.status));
        }

        let model_list: AnthropicModelList =
            serde_json::from_slice(&res.body).map_err(|_| Error::Unknown)?;

        Ok(model_list.models)
    }

    pub async fn get_model(&self, token: &str, model: &str) -> Result<ModelDesc, Error> {
        let req = HttpRequest {
            method: "GET".to_string(),
            url: format!(
                "{}/v1/models/{}",
                self.base_url.trim_end_matches('/'),
                model
            ),
            headers: vec![
                ("x-api-key".to_string(), token.to_string()),
                ("anthropic-version".to_string(), "2023-06-01".to_string()),
            ],
            body: vec![],
        };

        let res = self.transport.0.request(req).await?;
        if !(200..300).contains(&res.status) {
            return Err(Error::ServerError(res.status));
        }

        let model_desc: ModelDesc = serde_json::from_slice(&res.body).map_err(|_| Error::Unknown)?;

        Ok(model_desc)
    }

    pub async fn get_completion(
        &self,
        token: &str,
        request: CompletionRequest,
    ) -> Result<Completion, Error> {
        let req_body = AnthropicCompletionRequest {
            model: &request.model,
            max_tokens: request.max_tokens.unwrap_or(8192),
            messages: &request.messages,
            system: None,
            stream: None,
        };

        let body = serde_json::to_vec(&req_body).map_err(|_| Error::Unknown)?;

        let req = HttpRequest {
            method: "POST".to_string(),
            url: format!("{}/v1/messages", self.base_url.trim_end_matches('/')),
            headers: vec![
                ("x-api-key".to_string(), token.to_string()),
                ("anthropic-version".to_string(), "2023-06-01".to_string()),
                ("Content-Type".to_string(), "application/json".to_string()),
            ],
            body,
        };

        let res = self.transport.0.request(req).await?;
        if !(200..300).contains(&res.status) {
            return Err(Error::ServerError(res.status));
        }

        let anthropic: AnthropicCompletionResponse =
            serde_json::from_slice(&res.body).map_err(|_| Error::Unknown)?;

        let choices = anthropic
            .content
            .iter()
            .enumerate()
            .map(|(i, c)| CompletionChoice {
                message: None,
                text: Some(c.text.clone()),
                index: i as u16,
                delta: None,
                logprobs: None,
                tool_calls: None,
                finish_reason: anthropic.stop_reason.clone(),
            })
            .collect();

        Ok(Completion {
            id: anthropic.id,
            created: 0,
            model: anthropic.model,
            choices,
            usage: crate::Usage {
                prompt_tokens: anthropic.usage.input_tokens,
                completion_tokens: anthropic.usage.output_tokens,
                total_tokens: anthropic.usage.input_tokens + anthropic.usage.output_tokens,
            },
        })
    }

    pub fn stream_completion(
        &self,
        token: &str,
        request: CompletionRequest,
    ) -> Pin<Box<dyn Stream<Item = Result<StreamResponseChunk, Error>> + Send + 'static>> {
        #[derive(Deserialize)]
        struct StreamChunk {
            index: Option<u32>,
            delta: Option<AnthropicTextContent>,
            content_block: Option<AnthropicTextContent>,
            text: Option<String>,
        }

        let req_body = AnthropicCompletionRequest {
            model: &request.model,
            max_tokens: request.max_tokens.unwrap_or(1024),
            messages: &request.messages,
            system: None,
            stream: Some(true),
        };

        let body = match serde_json::to_vec(&req_body) {
            Ok(b) => b,
            Err(_) => {
                return Box::pin(futures::stream::once(async { Err(Error::Unknown) }));
            }
        };

        let req = HttpRequest {
            method: "POST".to_string(),
            url: format!("{}/v1/messages", self.base_url.trim_end_matches('/')),
            headers: vec![
                ("x-api-key".to_string(), token.to_string()),
                ("anthropic-version".to_string(), "2023-06-01".to_string()),
                ("Content-Type".to_string(), "application/json".to_string()),
            ],
            body,
        };

        let model = request.model.clone();
        let transport = self.transport.clone();

        let s = stream! {
            let mut upstream = transport.0.request_stream(req);
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

                while let Some(pos) = buf.windows(2).position(|w| w == b"\n\n") {
                    let event = buf.drain(..pos + 2).collect::<Vec<_>>();
                    let s = String::from_utf8_lossy(&event);
                    for line in s.lines() {
                        let line = line.trim();
                        if let Some(data) = line.strip_prefix("data: ") {
                            if data == "[DONE]" {
                                return;
                            }
                            let v = serde_json::from_str::<StreamChunk>(data);
                            if let Ok(chunk) = v {
                                let text = chunk.text
                                    .or_else(|| chunk.delta.as_ref().map(|d| d.text.clone()))
                                    .or_else(|| chunk.content_block.as_ref().map(|cb| cb.text.clone()));
                                if let Some(text) = text {
                                    let choice = CompletionChoice {
                                        message: None,
                                        text: Some(text),
                                        index: chunk.index.unwrap_or(0) as u16,
                                        delta: None,
                                        logprobs: None,
                                        tool_calls: None,
                                        finish_reason: None,
                                    };

                                    let response_chunk = StreamResponseChunk {
                                        id: format!("cmpl-{}", uuid::Uuid::new_v4()),
                                        object: "chat.completion.chunk".to_string(),
                                        created: std::time::SystemTime::now()
                                            .duration_since(std::time::UNIX_EPOCH)
                                            .unwrap_or_default()
                                            .as_secs(),
                                        model: model.clone(),
                                        system_fingerprint: None,
                                        choices: vec![choice],
                                    };

                                    yield Ok(response_chunk);
                                }
                            }
                        }
                    }
                }
            }
        };

        Box::pin(s)
    }
}
