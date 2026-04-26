use crate::{
    API, Completion, CompletionRequest, EmbeddingResponse, Error, ModelDesc, StreamResponseChunk,
};

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
}

impl std::fmt::Debug for OpenAI {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenAI")
            .field("base_url", &self.base_url)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn openai(base_url: &str) -> OpenAI {
        match OpenAI::new(base_url) {
            API::OpenAI(api) => api,
            _ => unreachable!(),
        }
    }

    #[test]
    fn endpoint_does_not_duplicate_v1() {
        assert_eq!(
            openai("https://example.com/v1").endpoint("/v1/models"),
            "https://example.com/v1/models"
        );
        assert_eq!(
            openai("https://example.com").endpoint("/v1/models"),
            "https://example.com/v1/models"
        );
    }

    #[test]
    fn parses_models_without_supported_parameters() {
        let body = br#"{"object":"list","data":[{"id":"openai/gpt-5.4","object":"model","created":1777057381,"owned_by":"proxy"}]}"#;
        let parsed: ModelListResponse = serde_json::from_slice(body).unwrap();

        assert_eq!(parsed.data[0].id, "openai/gpt-5.4");
        assert!(parsed.data[0].supported_parameters.is_empty());
    }
}

#[derive(Debug, Deserialize)]
struct ModelListResponse {
    data: Vec<ModelDesc>,
}

impl OpenAI {
    pub fn new(base_url: &str) -> API {
        API::OpenAI(OpenAI {
            base_url: base_url.to_string(),
        })
    }

    fn endpoint(&self, path: &str) -> String {
        let base = self.base_url.trim_end_matches('/');
        let path = if base.ends_with("/v1") {
            path.trim_start_matches("/v1")
        } else {
            path
        };
        format!("{}{}", base, path)
    }

    pub async fn list_models(&self, token: &str) -> Result<Vec<ModelDesc>, Error> {
        let req = Request::builder()
            .method("GET")
            .uri(&self.endpoint("/v1/models"))
            .header("Authorization", format!("Bearer {}", token))
            .body(Full::new(Bytes::new()))
            .map_err(|e| Error::InvalidRequest(e.to_string()))?;
        let (status, res_body) = crate::net::send_request(req).await?;

        if !(200..300).contains(&status) {
            return Err(Error::ServerError(status));
        }

        let model_list: ModelListResponse = serde_json::from_slice(&res_body)
            .map_err(|e| Error::ParsingError(format!("{:?}", e)))?;

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
            .uri(&self.endpoint("/v1/embeddings"))
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
            .uri(&self.endpoint(&format!("/v1/models/{}", model)))
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
        let body =
            serde_json::to_vec(&request).map_err(|e| Error::ParsingError(format!("{:?}", e)))?;

        let base = self.base_url.trim_end_matches('/');
        let path = if base.ends_with("/v1") {
            "/chat/completions"
        } else {
            "/v1/chat/completions"
        };

        let req = Request::builder()
            .method("POST")
            .uri(&format!("{}{}", base, path))
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

        let base = self.base_url.trim_end_matches('/');
        let path = if base.ends_with("/v1") {
            "/chat/completions"
        } else {
            "/v1/chat/completions"
        };

        let req = match Request::builder()
            .method("POST")
            .uri(&format!("{}{}", base, path))
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
            let stream = crate::net::stream_request(req, Ok::<Bytes, Error>);
            let mut upstream = stream.await;
            let mut decoder = crate::net::SseDecoder::new();
            let mut parsed = Vec::new();

            while let Some(item) = upstream.next().await {
                let chunk = match item {
                    Ok(data) => data,
                    Err(e) => {
                        yield Err(e);
                        return;
                    }
                };

                parsed.clear();
                let done = decoder.push(&chunk, |data| {
                    let chunk = serde_json::from_slice::<StreamResponseChunk>(data)
                        .map_err(|err| Error::ParsingError(format!("{}", err)))?;
                    parsed.push(chunk);
                    Ok(())
                });
                match done {
                    Ok(done) => {
                        for parsed_chunk in parsed.drain(..) {
                            yield Ok(parsed_chunk);
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
