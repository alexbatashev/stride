use crate::completion_request::CompletionRequest;
use crate::utils::{SharedExecutor, get_http_client};
use crate::{API, Completion, EmbeddingResponse, Error, ModelDesc, StreamResponseChunk};

use async_stream::stream;
use bytes::Bytes;
use futures::Stream;
use http_body_util::{BodyExt, Full};
use hyper::header::{AUTHORIZATION, CONTENT_TYPE};
use hyper::{Method, Request};
use serde::{Deserialize, Serialize};
use std::pin::Pin;

#[derive(Debug, Clone)]
pub struct OpenAI {
    base_url: String,
    executor: SharedExecutor,
}

#[derive(Debug, Deserialize)]
struct ModelListResponse {
    object: String,
    data: Vec<ModelDesc>,
}

impl OpenAI {
    pub fn new(base_url: &str, executor: SharedExecutor) -> API {
        API::OpenAI(OpenAI {
            base_url: base_url.to_string(),
            executor,
        })
    }

    pub async fn list_models(&self, token: &str) -> Result<Vec<ModelDesc>, Error> {
        let url = format!("{}/v1/models", self.base_url.trim_end_matches('/'));

        let req = Request::builder()
            .method(Method::GET)
            .uri(url)
            .header(AUTHORIZATION, format!("Bearer {}", token))
            .body(Full::new(Bytes::new()))
            .map_err(|_| Error::Unknown)?;

        let client = get_http_client(self.executor.clone())?;
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

        let model_list: ModelListResponse = serde_json::from_slice(&body_bytes)
            .map_err(|e| Error::ParsingError(format!("{:?}", e)))?;

        Ok(model_list.data)
    }

    pub async fn get_embeddings(
        &self,
        token: &str,
        input: &str,
        model: &str,
    ) -> Result<EmbeddingResponse, Error> {
        let url = format!("{}/v1/embeddings", self.base_url);

        #[derive(Debug, Clone, Serialize)]
        struct RequestData<'a> {
            input: &'a str,
            model: &'a str,
        }

        let json =
            serde_json::to_string(&RequestData { input, model }).map_err(|_| Error::Unknown)?;

        let auth_header = format!("Bearer {}", token);

        let req = Request::builder()
            .method(Method::POST)
            .uri(url)
            .header(CONTENT_TYPE, "application/json")
            .header(AUTHORIZATION, auth_header)
            .body(Full::new(Bytes::from(json)))
            .map_err(|e| Error::RequestError(format!("{:?}", e)))?;

        let client = get_http_client(self.executor.clone())?;

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

        let embeddings: EmbeddingResponse =
            serde_json::from_slice(&body_bytes).map_err(|_| Error::Unknown)?;

        Ok(embeddings)
    }

    pub async fn get_model(&self, token: &str, model: &str) -> Result<ModelDesc, Error> {
        let url = format!(
            "{}/v1/models/{}",
            self.base_url.trim_end_matches('/'),
            model
        );

        let req = Request::builder()
            .method(Method::GET)
            .uri(url)
            .header(AUTHORIZATION, format!("Bearer {}", token))
            .body(Full::new(Bytes::new()))
            .map_err(|_| Error::Unknown)?;

        let client = get_http_client(self.executor.clone())?;
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
        let url = format!(
            "{}/v1/chat/completions",
            self.base_url.trim_end_matches('/')
        );

        let json = serde_json::to_string(&request).map_err(|_| Error::Unknown)?;

        let auth_header = format!("Bearer {}", token);

        let req = Request::builder()
            .method(Method::POST)
            .uri(url)
            .header(CONTENT_TYPE, "application/json")
            .header(AUTHORIZATION, auth_header)
            .body(Full::new(Bytes::from(json)))
            .map_err(|e| Error::RequestError(format!("{:?}", e)))?;

        let client = get_http_client(self.executor.clone())?;
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
            // TODO improve this error reporting later.
            return Err(Error::ServerError(status.as_u16()));
        }

        let completion: Completion = serde_json::from_slice(&body_bytes)
            .map_err(|e| Error::ParsingError(format!("{:?}", e)))?;

        Ok(completion)
    }

    pub fn stream_completion(
        &self,
        token: &str,
        request: CompletionRequest,
    ) -> Pin<Box<dyn Stream<Item = Result<StreamResponseChunk, Error>> + Send + 'static>> {
        let base_url = self.base_url.clone();
        let token = token.to_owned();
        let executor = self.executor.clone();

        let s = stream! {
            let url = format!(
                "{}/v1/chat/completions",
                base_url.trim_end_matches('/')
            );

            let json = serde_json::to_string(&request).map_err(|e| Error::ParsingError(format!("{}", e)))?;

            let auth_header = format!("Bearer {}", token);

            let req = Request::builder()
                .method(Method::POST)
                .uri(url)
                .header(CONTENT_TYPE, "application/json")
                .header(AUTHORIZATION, auth_header)
                .body(Full::new(Bytes::from(json)))
                .map_err(|e| Error::RequestError(format!("{}", e)))?;

            let client = get_http_client(executor).map_err(|e| Error::RequestError(format!("{}", e)))?;
            let mut res = client.request(req).await.map_err(|e| Error::RequestError(format!("{}", e)))?;
            if !res.status().is_success() {
                yield Err(Error::ServerError(res.status().as_u16()));
                return;
            }

            let mut buf = Vec::new();

            // iterate frames with hyper_util::BodyExt
            while let Some(frame) = res.frame().await {
                let frame = match frame {
                    Ok(f) if f.is_data() => f.into_data().unwrap(),
                    _ => continue
                };
                buf.extend_from_slice(&frame);

                // Try to split buffer on double-newline as separator for events
                while let Some(pos) = buf.windows(2).position(|w| w == b"\n\n") {
                    let event = buf.drain(..pos + 2).collect::<Vec<_>>();
                    let s = String::from_utf8_lossy(&event);
                    for line in s.lines() {
                        if let Some(data) = line.strip_prefix("data: ") {
                            if data == "[DONE]" {
                                return; // End stream on [DONE]
                            }
                            let json = serde_json::from_str::<StreamResponseChunk>(data);
                            match json {
                                Ok(chunk) => yield Ok(chunk),
                                Err(err) => yield Err(Error::ParsingError(format!("{}", err)))
                            }
                        }
                    }
                }
            }
        };
        Box::pin(s)
    }
}
