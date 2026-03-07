mod types;

use crate::utils::TransportHandle;

use crate::{
    API, Completion, CompletionChoice, CompletionRequest, Delta, EmbeddingData, EmbeddingResponse,
    Error, Message, ModelDesc, ModelList, StreamResponseChunk, Usage,
};
use bytes::Bytes;
use http_body_util::Full;
use hyper::Request;

use async_stream::stream;
use futures::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use std::pin::Pin;
use uuid::Uuid;

use types::*;

#[derive(Clone)]
pub struct Ollama {
    base_url: String,
}

impl std::fmt::Debug for Ollama {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ollama")
            .field("base_url", &self.base_url)
            .finish()
    }
}

impl Ollama {
    pub fn new(base_url: &str) -> API {
        API::Ollama(Ollama {
            base_url: base_url.to_string(),
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

        let model_list: ModelList<OllamaModel> = serde_json::from_slice(&res_body)
            .map_err(|e| Error::ParsingError(format!("{:?}", e)))?;

        Ok(model_list
            .models
            .into_iter()
            .map(Into::<ModelDesc>::into)
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
            ..Default::default()
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

        let mut embeddings: OllamaEmbeddingsResponse = serde_json::from_slice(&res_body)
            .map_err(|e| Error::ParsingError(format!("{:?}", e)))?;

        Ok(embeddings.into())
    }

    pub async fn get_completion(
        &self,
        _token: &str,
        request: CompletionRequest,
    ) -> Result<Completion, Error> {
        let chat_request: OllamaChatRequest = (&request).into();
        let body = serde_json::to_vec(&chat_request).map_err(|_| Error::Unknown)?;

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

        let message: OllamaMessageResponse = serde_json::from_slice(&res_body).map_err(|e| {
            Error::ParsingError(format!("Failed to parse upstream response: {:?}", e))
        })?;

        Ok(message.into())
    }

    pub fn stream_completion(
        &self,
        _token: &str,
        request: CompletionRequest,
    ) -> Pin<Box<dyn Stream<Item = Result<StreamResponseChunk, Error>> + Send + 'static>> {
        let chat_request: OllamaChatRequest = (&request).into();
        let chat_request = chat_request.stream();
        let body = match serde_json::to_vec(&chat_request) {
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
            let mut upstream = crate::net::stream_request(req, Ok::<Bytes, Error>).await;
            let mut decoder = crate::net::NdjsonDecoder::new();
            let mut parsed = Vec::new();

            while let Some(item) = upstream.next().await {
                let data = match item {
                    Ok(d) => d,
                    Err(e) => {
                        yield Err(e);
                        return;
                    }
                };
                parsed.clear();
                let parse_res = decoder.push(&data, |line| {
                    let data = serde_json::from_slice::<OllamaMessageResponse>(line)
                        .map_err(|e| Error::ParsingError(format!("{}", e)))?;
                    parsed.push(data);
                    Ok(())
                });

                if let Err(err) = parse_res {
                    yield Err(err);
                    return;
                }

                for data in parsed.drain(..) {
                    let done = data.done;
                    yield Ok(data.into());
                    if done {
                        return;
                    }
                }
            }

        };
        Box::pin(s)
    }
}
