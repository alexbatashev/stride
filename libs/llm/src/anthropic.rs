use std::pin::Pin;

use async_stream::stream;
use bytes::Bytes;
use futures::Stream;
use http_body_util::{BodyExt, Full};
use hyper::header::CONTENT_TYPE;
use hyper::{Method, Request};
use hyper_rustls::ConfigBuilderExt;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use serde::{Deserialize, Serialize};
use uuid;

use crate::completion_request::CompletionRequest;
use crate::{API, Completion, CompletionChoice, Error, Message, ModelDesc, StreamResponseChunk};

#[derive(Debug, Clone)]
pub struct Anthropic {
    base_url: String,
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
    pub fn new(base_url: &str) -> API {
        API::Anthropic(Anthropic {
            base_url: base_url.to_string(),
        })
    }

    pub async fn list_models(&self, token: &str) -> Result<Vec<ModelDesc>, Error> {
        #[derive(Debug, Deserialize)]
        struct AnthropicModelList {
            models: Vec<ModelDesc>,
        }

        let url = format!("{}/v1/models", self.base_url.trim_end_matches('/'));

        let tls = rustls::ClientConfig::builder()
            .with_native_roots()
            .map_err(|_| Error::Unknown)?
            .with_no_client_auth();

        let https = hyper_rustls::HttpsConnectorBuilder::new()
            .with_tls_config(tls)
            .https_or_http()
            .enable_http1()
            .build();

        let req = Request::builder()
            .method(Method::GET)
            .uri(url)
            .header("x-api-key", token)
            .header("anthropic-version", "2023-06-01")
            .body(Full::new(Bytes::new()))
            .map_err(|_| Error::Unknown)?;

        let client: Client<_, Full<Bytes>> = Client::builder(TokioExecutor::new()).build(https);
        let res = client.request(req).await.map_err(|_| Error::Unknown)?;
        let status = res.status();

        let body_bytes = res
            .into_body()
            .collect()
            .await
            .map_err(|_| Error::Unknown)?
            .to_bytes();

        if !status.is_success() {
            return Err(Error::Unknown);
        }

        let model_list: AnthropicModelList =
            serde_json::from_slice(&body_bytes).map_err(|_| Error::Unknown)?;

        Ok(model_list.models)
    }

    pub async fn get_model(&self, token: &str, model: &str) -> Result<ModelDesc, Error> {
        let url = format!(
            "{}/v1/models/{}",
            self.base_url.trim_end_matches('/'),
            model
        );

        let tls = rustls::ClientConfig::builder()
            .with_native_roots()
            .map_err(|_| Error::Unknown)?
            .with_no_client_auth();

        let https = hyper_rustls::HttpsConnectorBuilder::new()
            .with_tls_config(tls)
            .https_or_http()
            .enable_http1()
            .build();

        let req = Request::builder()
            .method(Method::GET)
            .uri(url)
            .header("x-api-key", token)
            .header("anthropic-version", "2023-06-01")
            .body(Full::new(Bytes::new()))
            .map_err(|_| Error::Unknown)?;

        let client: Client<_, Full<Bytes>> = Client::builder(TokioExecutor::new()).build(https);
        let res = client.request(req).await.map_err(|_| Error::Unknown)?;
        let status = res.status();

        let body_bytes = res
            .into_body()
            .collect()
            .await
            .map_err(|_| Error::Unknown)?
            .to_bytes();

        if !status.is_success() {
            return Err(Error::Unknown);
        }

        let model_desc: ModelDesc =
            serde_json::from_slice(&body_bytes).map_err(|_| Error::Unknown)?;

        Ok(model_desc)
    }

    pub async fn get_completion(
        &self,
        token: &str,
        request: CompletionRequest,
    ) -> Result<Completion, Error> {
        let url = format!("{}/v1/messages", self.base_url.trim_end_matches('/'));

        let req_body = AnthropicCompletionRequest {
            model: &request.model,
            max_tokens: request.max_tokens.unwrap_or(8192),
            messages: &request.messages,
            system: None,
            stream: None,
        };

        let json = serde_json::to_string(&req_body).map_err(|_| Error::Unknown)?;

        let req = Request::builder()
            .method(Method::POST)
            .uri(url)
            .header("x-api-key", token)
            .header("anthropic-version", "2023-06-01")
            .header(CONTENT_TYPE, "application/json")
            .body(Full::new(Bytes::from(json)))
            .map_err(|_| Error::Unknown)?;

        let tls = rustls::ClientConfig::builder()
            .with_native_roots()
            .map_err(|_| Error::Unknown)?
            .with_no_client_auth();

        let https = hyper_rustls::HttpsConnectorBuilder::new()
            .with_tls_config(tls)
            .https_or_http()
            .enable_http1()
            .build();

        let client: Client<_, Full<Bytes>> = Client::builder(TokioExecutor::new()).build(https);
        let res = client.request(req).await.map_err(|_| Error::Unknown)?;
        let status = res.status();

        let body_bytes = res
            .into_body()
            .collect()
            .await
            .map_err(|_| Error::Unknown)?
            .to_bytes();

        if !status.is_success() {
            return Err(Error::Unknown);
        }

        let anthropic: AnthropicCompletionResponse =
            serde_json::from_slice(&body_bytes).map_err(|_| Error::Unknown)?;

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

        let base_url = self.base_url.clone();
        let token = token.to_owned();

        let s = stream! {
            let url = format!("{}/v1/messages", base_url.trim_end_matches('/'));

            let req_body = AnthropicCompletionRequest {
                model: &request.model,
                max_tokens: request.max_tokens.unwrap_or(1024),
                messages: &request.messages,
                system: None,
                stream: Some(true),
            };

            let json = serde_json::to_string(&req_body)
                .map_err(|_| Error::Unknown)?;

            let req = Request::builder()
                .method(Method::POST)
                .uri(url)
                .header("x-api-key", token)
                .header("anthropic-version", "2023-06-01")
                .header(CONTENT_TYPE, "application/json")
                .body(Full::new(Bytes::from(json)))
                .map_err(|_| Error::Unknown)?;

            let tls = rustls::ClientConfig::builder()
                .with_native_roots()
                .map_err(|_| Error::Unknown)?
                .with_no_client_auth();

            let https = hyper_rustls::HttpsConnectorBuilder::new()
                .with_tls_config(tls)
                .https_or_http()
                .enable_http1()
                .build();

            let client: Client<_, Full<Bytes>> = Client::builder(TokioExecutor::new()).build(https);
            let mut res = client.request(req).await.map_err(|_| Error::Unknown)?;
            if !res.status().is_success() {
                yield Err(Error::Unknown);
                return;
            }

            let mut buf = Vec::new();

            while let Some(frame) = res.frame().await {
                let frame = match frame {
                    Ok(f) if f.is_data() => f.into_data().unwrap(),
                    _ => continue
                };
                buf.extend_from_slice(&frame);

                // Each SSE event ends with \n\n
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
                                // Adapt to StreamResponseChunk structure
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
                                        finish_reason: None,
                                    };

                                    let response_chunk = StreamResponseChunk {
                                        id: format!("cmpl-{}", uuid::Uuid::new_v4()), // Generate a unique ID
                                        object: "chat.completion.chunk".to_string(),
                                        created: std::time::SystemTime::now()
                                            .duration_since(std::time::UNIX_EPOCH)
                                            .unwrap_or_default()
                                            .as_secs(),
                                        model: request.model.clone(),
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
