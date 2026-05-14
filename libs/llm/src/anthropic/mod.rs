mod types;

use std::pin::Pin;

use async_stream::stream;
use futures::{Stream, StreamExt};

use crate::{API, Completion, CompletionRequest, Error, ModelDesc, StreamResponseChunk};

use bytes::Bytes;
use http_body_util::Full;
use hyper::Request;

use types::*;

#[derive(Clone)]
pub struct Anthropic {
    base_url: String,
}

impl std::fmt::Debug for Anthropic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Anthropic")
            .field("base_url", &self.base_url)
            .finish()
    }
}

impl Anthropic {
    pub fn new(base_url: &str) -> API {
        API::Anthropic(Anthropic {
            base_url: base_url.to_string(),
        })
    }

    pub async fn list_models(&self, token: &str) -> Result<Vec<ModelDesc>, Error> {
        let req = Request::builder()
            .method("GET")
            .uri(&format!(
                "{}/v1/models",
                self.base_url.trim_end_matches('/')
            ))
            .header("x-api-key", token)
            .header("anthropic-version", "2023-06-01")
            .body(Full::new(Bytes::new()))
            .map_err(|e| Error::InvalidRequest(e.to_string()))?;
        let (status, res_body) = tinynet::send_request(req).await?;
        if !(200..300).contains(&status) {
            return Err(Error::ServerError(status));
        }

        let model_list: AnthropicModelList =
            serde_json::from_slice(&res_body).map_err(|_| Error::Unknown)?;

        Ok(model_list.models)
    }

    pub async fn get_model(&self, token: &str, model: &str) -> Result<ModelDesc, Error> {
        let req = Request::builder()
            .method("GET")
            .uri(&format!(
                "{}/v1/models/{}",
                self.base_url.trim_end_matches('/'),
                model
            ))
            .header("x-api-key", token)
            .header("anthropic-version", "2023-06-01")
            .body(Full::new(Bytes::new()))
            .map_err(|e| Error::InvalidRequest(e.to_string()))?;
        let (status, res_body) = tinynet::send_request(req).await?;
        if !(200..300).contains(&status) {
            return Err(Error::ServerError(status));
        }

        let model_desc: ModelDesc =
            serde_json::from_slice(&res_body).map_err(|_| Error::Unknown)?;

        Ok(model_desc)
    }

    pub async fn get_completion(
        &self,
        token: &str,
        request: CompletionRequest,
    ) -> Result<Completion, Error> {
        let req_body: AnthropicCompletionRequest = (&request).into();

        let body = serde_json::to_vec(&req_body).map_err(|_| Error::Unknown)?;

        let req = Request::builder()
            .method("POST")
            .uri(&format!(
                "{}/v1/messages",
                self.base_url.trim_end_matches('/')
            ))
            .header("x-api-key", token)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .body(Full::new(Bytes::from(body)))
            .map_err(|e| Error::InvalidRequest(e.to_string()))?;
        let (status, res_body) = tinynet::send_request(req).await?;
        if !(200..300).contains(&status) {
            return Err(Error::ServerError(status));
        }

        let anthropic: AnthropicCompletionResponse =
            serde_json::from_slice(&res_body).map_err(|_| Error::Unknown)?;

        Ok(anthropic.into())
    }

    pub fn stream_completion(
        &self,
        token: &str,
        request: CompletionRequest,
    ) -> Pin<Box<dyn Stream<Item = Result<StreamResponseChunk, Error>> + Send + 'static>> {
        let req_body: AnthropicCompletionRequest = (&request).into();
        let req_body = req_body.stream_with_max_tokens(request.max_tokens.unwrap_or(1024));

        let body = match serde_json::to_vec(&req_body) {
            Ok(b) => b,
            Err(_) => {
                return Box::pin(futures::stream::once(async { Err(Error::Unknown) }));
            }
        };

        let req = match Request::builder()
            .method("POST")
            .uri(&format!(
                "{}/v1/messages",
                self.base_url.trim_end_matches('/')
            ))
            .header("x-api-key", token)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .body(Full::new(Bytes::from(body)))
            .map_err(|e| Error::InvalidRequest(e.to_string()))
        {
            Ok(req) => req,
            Err(err) => return Box::pin(futures::stream::once(async move { Err(err) })),
        };

        let model = request.model.clone();

        let s = stream! {
            let mut upstream = tinynet::stream_request(req).await;
            let mut decoder = tinynet::SseDecoder::new();
            let mut parsed = Vec::new();

            while let Some(item) = upstream.next().await {
                let data = match item {
                    Ok(d) => d,
                    Err(e) => {
                        yield Err(e.into());
                        return;
                    }
                };
                parsed.clear();
                let done = decoder.push(&data, |event_data| {
                    let chunk = serde_json::from_slice::<AnthropicStreamChunk>(event_data)
                        .map_err(|e| Error::ParsingError(format!("{}", e)))?;
                    let chunk = AnthropicStreamChunkWithModel {
                        model: model.clone(),
                        chunk,
                    };
                    if let Ok(chunk) = StreamResponseChunk::try_from(chunk) {
                        parsed.push(chunk);
                    }
                    Ok(())
                });

                match done {
                    Ok(done) => {
                        for chunk in parsed.drain(..) {
                            yield Ok(chunk);
                        }
                        if done {
                            return;
                        }
                    }
                    Err(err) => {
                        yield Err(err);
                        return;
                    }
                }
            }
        };

        Box::pin(s)
    }
}
