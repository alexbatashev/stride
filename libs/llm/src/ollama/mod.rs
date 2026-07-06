mod types;

use crate::{
    Completion, CompletionRequest, EmbeddingResponse, Error, ImageSource, ModelDesc, ModelList,
    StreamResponseChunk,
};
use base64::Engine;
use bytes::Bytes;
use http_body_util::Full;
use hyper::{Request, http::request::Builder};

use async_stream::stream;
use futures::{Stream, StreamExt};
use serde::Serialize;
use std::{pin::Pin, time::Duration};

use types::*;

const MAX_IMAGE_BYTES: usize = 20 * 1024 * 1024;
const CHAT_TIMEOUT_SECS: u64 = 120;

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
    pub fn new(base_url: &str) -> Self {
        Ollama {
            base_url: base_url.to_string(),
        }
    }

    pub fn cloud(base_url: &str) -> Self {
        Self::new(base_url)
    }

    fn endpoint(&self, path: &str) -> String {
        let base = self.base_url.trim_end_matches('/');
        if base.ends_with("/api") {
            return format!("{}{}", base, path.trim_start_matches("/api"));
        }
        if let Some(base) = base.strip_suffix("/v1") {
            return format!("{base}{path}");
        }
        format!("{base}{path}")
    }

    fn request_builder(&self, method: &str, path: &str, token: &str) -> Builder {
        let builder = Request::builder().method(method).uri(self.endpoint(path));
        let token = token.trim();
        if token.is_empty() || token == "-" {
            builder
        } else {
            builder.header("Authorization", format!("Bearer {token}"))
        }
    }

    pub async fn list_models(&self, token: &str) -> Result<Vec<ModelDesc>, Error> {
        let req = self
            .request_builder("GET", "/api/tags", token)
            .body(Full::new(Bytes::new()))
            .map_err(|e| Error::InvalidRequest(e.to_string()))?;
        let (status, res_body) = tinynet::send_request(req).await?;
        if !(200..300).contains(&status) {
            return Err(server_error(status, &res_body));
        }

        let model_list: ModelList<OllamaModel> = serde_json::from_slice(&res_body)
            .map_err(|e| Error::ParsingError(format!("{:?}", e)))?;

        Ok(model_list
            .models
            .into_iter()
            .map(Into::<ModelDesc>::into)
            .collect())
    }

    pub async fn get_model(&self, token: &str, model: &str) -> Result<ModelDesc, Error> {
        #[derive(Serialize, Debug, Clone)]
        struct Body<'a> {
            model: &'a str,
        }

        let body = serde_json::to_vec(&Body { model }).map_err(|_| Error::Unknown)?;

        let req = self
            .request_builder("POST", "/api/show", token)
            .header("Content-Type", "application/json")
            .body(Full::new(Bytes::from(body)))
            .map_err(|e| Error::InvalidRequest(e.to_string()))?;
        let (status, res_body) = tinynet::send_request(req).await?;
        if !(200..300).contains(&status) {
            return Err(server_error(status, &res_body));
        }

        Ok(ModelDesc {
            id: model.to_string(),
            ..Default::default()
        })
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

        let body = serde_json::to_vec(&RequestData { input, model }).map_err(|_| Error::Unknown)?;

        let req = self
            .request_builder("POST", "/api/embed", token)
            .header("Content-Type", "application/json")
            .body(Full::new(Bytes::from(body)))
            .map_err(|e| Error::InvalidRequest(e.to_string()))?;
        let (status, res_body) = tinynet::send_request(req).await?;
        if !(200..300).contains(&status) {
            return Err(server_error(status, &res_body));
        }

        let embeddings: OllamaEmbeddingsResponse = serde_json::from_slice(&res_body)
            .map_err(|e| Error::ParsingError(format!("{:?}", e)))?;

        Ok(embeddings.into())
    }

    pub async fn get_completion(
        &self,
        token: &str,
        request: CompletionRequest,
    ) -> Result<Completion, Error> {
        let request = inline_remote_images(request).await?;
        let chat_request: OllamaChatRequest = (&request).into();
        let body = serde_json::to_vec(&chat_request).map_err(|_| Error::Unknown)?;

        let req = self
            .request_builder("POST", "/api/chat", token)
            .header("Content-Type", "application/json")
            .body(Full::new(Bytes::from(body)))
            .map_err(|e| Error::InvalidRequest(e.to_string()))?;
        let (status, res_body) =
            tinynet::send_request_with_timeout(req, Duration::from_secs(CHAT_TIMEOUT_SECS)).await?;
        if !(200..300).contains(&status) {
            return Err(server_error(status, &res_body));
        }

        let message: OllamaMessageResponse = serde_json::from_slice(&res_body).map_err(|e| {
            Error::ParsingError(format!("Failed to parse upstream response: {:?}", e))
        })?;

        Ok(message.into())
    }

    pub fn stream_completion(
        &self,
        token: &str,
        request: CompletionRequest,
    ) -> Pin<Box<dyn Stream<Item = Result<StreamResponseChunk, Error>> + Send + 'static>> {
        let client = self.clone();
        let token = token.to_string();

        let s = stream! {
            let request = match inline_remote_images(request).await {
                Ok(request) => request,
                Err(err) => {
                    yield Err(err);
                    return;
                }
            };
            let chat_request: OllamaChatRequest = (&request).into();
            let chat_request = chat_request.stream();
            let body = match serde_json::to_vec(&chat_request) {
                Ok(b) => b,
                Err(_) => {
                    yield Err(Error::Unknown);
                    return;
                }
            };

            let req = match client
                .request_builder("POST", "/api/chat", &token)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(body)))
                .map_err(|e| Error::InvalidRequest(e.to_string()))
            {
                Ok(req) => req,
                Err(err) => {
                    yield Err(err);
                    return;
                }
            };

            let mut upstream = tinynet::stream_request(req).await;
            let mut decoder = tinynet::NdjsonDecoder::new();
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

async fn inline_remote_images(mut request: CompletionRequest) -> Result<CompletionRequest, Error> {
    for message in &mut request.messages {
        let Some(images) = &mut message.images else {
            continue;
        };
        for image in images {
            inline_remote_image(image).await?;
        }
    }
    Ok(request)
}

async fn inline_remote_image(image: &mut ImageSource) -> Result<(), Error> {
    if image.data.is_some() {
        return Ok(());
    }

    let Some(url) = image.url.as_deref() else {
        return Ok(());
    };

    if let Some((_, data)) = url.split_once(";base64,") {
        image.data = Some(data.to_string());
        image.url = None;
        return Ok(());
    }

    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(Error::InvalidRequest(
            "Ollama image inputs must be base64 data or an http(s) URL".to_string(),
        ));
    }

    let req = Request::builder()
        .method("GET")
        .uri(url)
        .body(Full::new(Bytes::new()))
        .map_err(|e| Error::InvalidRequest(e.to_string()))?;
    let (status, body) = tinynet::send_request(req).await?;
    if !(200..300).contains(&status) {
        return Err(server_error(status, &body));
    }
    if body.len() > MAX_IMAGE_BYTES {
        return Err(Error::InvalidRequest(format!(
            "image is too large for Ollama request: {} bytes",
            body.len()
        )));
    }

    image.data = Some(base64::engine::general_purpose::STANDARD.encode(body));
    image.url = None;
    Ok(())
}

fn server_error(status: u16, body: &[u8]) -> Error {
    let message = String::from_utf8_lossy(body).trim().to_string();
    if message.is_empty() {
        Error::ServerError(status)
    } else {
        Error::ServerErrorWithMessage(status, message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_accepts_root_api_and_legacy_v1_base_urls() {
        assert_eq!(
            Ollama::new("https://ollama.com").endpoint("/api/chat"),
            "https://ollama.com/api/chat"
        );
        assert_eq!(
            Ollama::new("https://ollama.com/api").endpoint("/api/chat"),
            "https://ollama.com/api/chat"
        );
        assert_eq!(
            Ollama::new("http://localhost:11434/v1").endpoint("/api/chat"),
            "http://localhost:11434/api/chat"
        );
    }

    #[test]
    fn request_builder_adds_auth_only_for_real_tokens() {
        let client = Ollama::cloud("https://ollama.com");
        let req = client
            .request_builder("GET", "/api/tags", "ollama-token")
            .body(Full::new(Bytes::new()))
            .unwrap();
        assert_eq!(
            req.headers().get("Authorization").unwrap(),
            "Bearer ollama-token"
        );

        let req = client
            .request_builder("GET", "/api/tags", "-")
            .body(Full::new(Bytes::new()))
            .unwrap();
        assert!(req.headers().get("Authorization").is_none());
    }
}
