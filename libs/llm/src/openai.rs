use crate::utils::TransportHandle;
use crate::{API, Completion, CompletionRequest, EmbeddingResponse, Error, ModelDesc, StreamResponseChunk};

use async_stream::stream;
use bytes::Bytes;
use futures::{Stream, StreamExt};
use http_body_util::Full;
use hyper::Request;
use serde::{Deserialize, Serialize};
use std::pin::Pin;

#[derive(Clone)]
pub struct OpenAI {
    base_url: String,
    _transport: TransportHandle,
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
            _transport: transport,
        })
    }

    pub async fn list_models(&self, token: &str) -> Result<Vec<ModelDesc>, Error> {
        let req = Request::builder()
            .method("GET")
            .uri(&format!("{}/v1/models", self.base_url.trim_end_matches('/')))
            .header("Authorization", format!("Bearer {}", token))
            .body(Full::new(Bytes::new()))
            .map_err(|e| Error::InvalidRequest(e.to_string()))?;
        let (status, res_body) = crate::net::send_request(req).await?;

        if !(200..300).contains(&status) {
            return Err(Error::ServerError(status));
        }

        let model_list: ModelListResponse =
            serde_json::from_slice(&res_body).map_err(|e| Error::ParsingError(format!("{:?}", e)))?;

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

        let req = Request::builder()
            .method("POST")
            .uri(&format!("{}/v1/embeddings", self.base_url.trim_end_matches('/')))
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", token))
            .body(Full::new(Bytes::from(body)))
            .map_err(|e| Error::InvalidRequest(e.to_string()))?;
        let (status, res_body) = crate::net::send_request(req).await?;

        if !(200..300).contains(&status) {
            return Err(Error::ServerError(status));
        }

        serde_json::from_slice(&res_body).map_err(|e| Error::ParsingError(format!("{:?}", e)))
    }

    pub async fn get_model(&self, token: &str, model: &str) -> Result<ModelDesc, Error> {
        let req = Request::builder()
            .method("GET")
            .uri(&format!("{}/v1/models/{}", self.base_url.trim_end_matches('/'), model))
            .header("Authorization", format!("Bearer {}", token))
            .body(Full::new(Bytes::new()))
            .map_err(|e| Error::InvalidRequest(e.to_string()))?;
        let (status, res_body) = crate::net::send_request(req).await?;

        if !(200..300).contains(&status) {
            return Err(Error::ServerError(status));
        }

        serde_json::from_slice(&res_body).map_err(|e| Error::ParsingError(format!("{:?}", e)))
    }

    pub async fn get_completion(
        &self,
        token: &str,
        request: CompletionRequest,
    ) -> Result<Completion, Error> {
        let body = serde_json::to_vec(&request).map_err(|e| Error::ParsingError(format!("{:?}", e)))?;

        let req = Request::builder()
            .method("POST")
            .uri(&format!("{}/v1/chat/completions", self.base_url.trim_end_matches('/')))
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", token))
            .body(Full::new(Bytes::from(body)))
            .map_err(|e| Error::InvalidRequest(e.to_string()))?;
        let (status, res_body) = crate::net::send_request(req).await?;

        if !(200..300).contains(&status) {
            return Err(Error::ServerError(status));
        }

        serde_json::from_slice(&res_body).map_err(|e| Error::ParsingError(format!("{:?}", e)))
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

        let req = match Request::builder()
            .method("POST")
            .uri(&format!("{}/v1/chat/completions", self.base_url.trim_end_matches('/')))
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", token))
            .body(Full::new(Bytes::from(req_body)))
            .map_err(|e| Error::InvalidRequest(e.to_string()))
        {
            Ok(req) => req,
            Err(err) => {
                return Box::pin(futures::stream::once(async move { Err(err) }));
            }
        };

        let s = stream! {
            let mut upstream = crate::net::stream_request(req, |data| Ok::<Vec<u8>, Error>(data.to_vec())).await;
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
