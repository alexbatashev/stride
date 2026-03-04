use crate::utils::{HttpRequest, TransportHandle};
use crate::{API, Completion, CompletionRequest, EmbeddingResponse, Error, ModelDesc, StreamResponseChunk};

use async_stream::stream;
use futures::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use std::pin::Pin;

#[derive(Clone)]
pub struct OpenAI {
    base_url: String,
    transport: TransportHandle,
}

impl std::fmt::Debug for OpenAI {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenAI")
            .field("base_url", &self.base_url)
            .finish()
    }
}

#[derive(Debug, Deserialize)]
struct ModelListResponse {
    data: Vec<ModelDesc>,
}

impl OpenAI {
    pub fn new(base_url: &str, transport: TransportHandle) -> API {
        API::OpenAI(OpenAI {
            base_url: base_url.to_string(),
            transport,
        })
    }

    pub async fn list_models(&self, token: &str) -> Result<Vec<ModelDesc>, Error> {
        let req = HttpRequest {
            method: "GET".to_string(),
            url: format!("{}/v1/models", self.base_url.trim_end_matches('/')),
            headers: vec![("Authorization".to_string(), format!("Bearer {}", token))],
            body: vec![],
        };

        let res = self.transport.0.request(req).await?;
        if !(200..300).contains(&res.status) {
            return Err(Error::ServerError(res.status));
        }

        let model_list: ModelListResponse =
            serde_json::from_slice(&res.body).map_err(|e| Error::ParsingError(format!("{:?}", e)))?;

        Ok(model_list.data)
    }

    pub async fn get_embeddings(
        &self,
        token: &str,
        input: &str,
        model: &str,
    ) -> Result<EmbeddingResponse, Error> {
        #[derive(Debug, Clone, Serialize)]
        struct RequestData<'a> {
            input: &'a str,
            model: &'a str,
        }

        let body = serde_json::to_vec(&RequestData { input, model })
            .map_err(|e| Error::ParsingError(format!("{:?}", e)))?;

        let req = HttpRequest {
            method: "POST".to_string(),
            url: format!("{}/v1/embeddings", self.base_url.trim_end_matches('/')),
            headers: vec![
                ("Content-Type".to_string(), "application/json".to_string()),
                ("Authorization".to_string(), format!("Bearer {}", token)),
            ],
            body,
        };

        let res = self.transport.0.request(req).await?;
        if !(200..300).contains(&res.status) {
            return Err(Error::ServerError(res.status));
        }

        serde_json::from_slice(&res.body).map_err(|e| Error::ParsingError(format!("{:?}", e)))
    }

    pub async fn get_model(&self, token: &str, model: &str) -> Result<ModelDesc, Error> {
        let req = HttpRequest {
            method: "GET".to_string(),
            url: format!(
                "{}/v1/models/{}",
                self.base_url.trim_end_matches('/'),
                model
            ),
            headers: vec![("Authorization".to_string(), format!("Bearer {}", token))],
            body: vec![],
        };

        let res = self.transport.0.request(req).await?;
        if !(200..300).contains(&res.status) {
            return Err(Error::ServerError(res.status));
        }

        serde_json::from_slice(&res.body).map_err(|e| Error::ParsingError(format!("{:?}", e)))
    }

    pub async fn get_completion(
        &self,
        token: &str,
        request: CompletionRequest,
    ) -> Result<Completion, Error> {
        let body = serde_json::to_vec(&request).map_err(|e| Error::ParsingError(format!("{:?}", e)))?;

        let req = HttpRequest {
            method: "POST".to_string(),
            url: format!("{}/v1/chat/completions", self.base_url.trim_end_matches('/')),
            headers: vec![
                ("Content-Type".to_string(), "application/json".to_string()),
                ("Authorization".to_string(), format!("Bearer {}", token)),
            ],
            body,
        };

        let res = self.transport.0.request(req).await?;
        if !(200..300).contains(&res.status) {
            return Err(Error::ServerError(res.status));
        }

        serde_json::from_slice(&res.body).map_err(|e| Error::ParsingError(format!("{:?}", e)))
    }

    pub fn stream_completion(
        &self,
        token: &str,
        request: CompletionRequest,
    ) -> Pin<Box<dyn Stream<Item = Result<StreamResponseChunk, Error>> + Send + 'static>> {
        let req_body = match serde_json::to_vec(&request) {
            Ok(v) => v,
            Err(err) => {
                return Box::pin(futures::stream::once(async move {
                    Err(Error::ParsingError(format!("{}", err)))
                }));
            }
        };

        let req = HttpRequest {
            method: "POST".to_string(),
            url: format!("{}/v1/chat/completions", self.base_url.trim_end_matches('/')),
            headers: vec![
                ("Content-Type".to_string(), "application/json".to_string()),
                ("Authorization".to_string(), format!("Bearer {}", token)),
            ],
            body: req_body,
        };

        let transport = self.transport.clone();

        let s = stream! {
            let mut upstream = transport.0.request_stream(req);
            let mut buf = Vec::new();

            while let Some(item) = upstream.next().await {
                let chunk = match item {
                    Ok(data) => data,
                    Err(e) => {
                        yield Err(e);
                        return;
                    }
                };

                buf.extend_from_slice(&chunk);
                while let Some(pos) = buf.windows(2).position(|w| w == b"\n\n") {
                    let event = buf.drain(..pos + 2).collect::<Vec<_>>();
                    let s = String::from_utf8_lossy(&event);
                    for line in s.lines() {
                        if let Some(data) = line.strip_prefix("data: ") {
                            if data == "[DONE]" {
                                return;
                            }
                            match serde_json::from_str::<StreamResponseChunk>(data) {
                                Ok(chunk) => yield Ok(chunk),
                                Err(err) => yield Err(Error::ParsingError(format!("{}", err))),
                            }
                        }
                    }
                }
            }
        };

        Box::pin(s)
    }
}
