use crate::{
    Completion, CompletionRequest, EmbeddingResponse, Error, ImageSource, ModelDesc,
    StreamResponseChunk,
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
        OpenAI::new(base_url)
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
    fn serializes_message_images_as_content_parts() {
        use crate::{ImageSource, Message, Role};

        let request = CompletionRequest {
            model: "gpt-vision".to_string(),
            messages: vec![Message {
                role: Role::User,
                content: "describe".to_string(),
                images: Some(vec![
                    ImageSource::url("https://example.com/a.png"),
                    ImageSource::base64("image/jpeg", "QUJD"),
                ]),
                ..Default::default()
            }],
            ..Default::default()
        };

        let body = serialize_request(&request).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let message = &value["messages"][0];

        assert!(message.get("images").is_none());
        let parts = message["content"].as_array().unwrap();
        assert_eq!(parts[0]["type"], "text");
        assert_eq!(parts[0]["text"], "describe");
        assert_eq!(parts[1]["type"], "image_url");
        assert_eq!(parts[1]["image_url"]["url"], "https://example.com/a.png");
        assert_eq!(parts[2]["type"], "image_url");
        assert_eq!(parts[2]["image_url"]["url"], "data:image/jpeg;base64,QUJD");
    }

    #[test]
    fn leaves_text_only_messages_unchanged() {
        use crate::{Message, Role};

        let request = CompletionRequest {
            model: "gpt".to_string(),
            messages: vec![Message {
                role: Role::User,
                content: "hello".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        };

        let body = serialize_request(&request).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["messages"][0]["content"], "hello");
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

/// Serializes a request to OpenAI's wire format, rewriting any message that
/// carries images into the `content` parts array (text part plus one
/// `image_url` part per image) that the chat completions API expects.
fn serialize_request(request: &CompletionRequest) -> Result<Vec<u8>, Error> {
    let mut body =
        serde_json::to_value(request).map_err(|e| Error::ParsingError(format!("{:?}", e)))?;

    if let Some(messages) = body.get_mut("messages").and_then(|m| m.as_array_mut()) {
        for message in messages {
            rewrite_message_images(message);
        }
    }

    serde_json::to_vec(&body).map_err(|e| Error::ParsingError(format!("{:?}", e)))
}

fn rewrite_message_images(message: &mut serde_json::Value) {
    let Some(object) = message.as_object_mut() else {
        return;
    };
    let Some(images) = object.remove("images") else {
        return;
    };
    let images: Vec<ImageSource> = match serde_json::from_value(images) {
        Ok(images) => images,
        Err(_) => return,
    };
    if images.is_empty() {
        return;
    }

    let mut parts = Vec::new();
    if let Some(text) = object.get("content").and_then(|c| c.as_str())
        && !text.is_empty()
    {
        parts.push(serde_json::json!({ "type": "text", "text": text }));
    }
    for image in images {
        if let Some(url) = image.as_request_url() {
            parts.push(serde_json::json!({
                "type": "image_url",
                "image_url": { "url": url },
            }));
        }
    }

    object.insert("content".to_string(), serde_json::Value::Array(parts));
}

impl OpenAI {
    pub fn new(base_url: &str) -> Self {
        OpenAI {
            base_url: base_url.to_string(),
        }
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
            .uri(self.endpoint("/v1/models"))
            .header("Authorization", format!("Bearer {}", token))
            .body(Full::new(Bytes::new()))
            .map_err(|e| Error::InvalidRequest(e.to_string()))?;
        let (status, res_body) = tinynet::send_request(req).await?;

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
            .uri(self.endpoint("/v1/embeddings"))
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", token))
            .body(Full::new(Bytes::from(body)))
            .map_err(|e| Error::InvalidRequest(e.to_string()))?;
        let (status, res_body) = tinynet::send_request(req).await?;

        if !(200..300).contains(&status) {
            return Err(Error::ServerError(status));
        }

        serde_json::from_slice(&res_body).map_err(|e| Error::ParsingError(format!("{:?}", e)))
    }

    pub async fn get_model(&self, token: &str, model: &str) -> Result<ModelDesc, Error> {
        let req = Request::builder()
            .method("GET")
            .uri(self.endpoint(&format!("/v1/models/{}", model)))
            .header("Authorization", format!("Bearer {}", token))
            .body(Full::new(Bytes::new()))
            .map_err(|e| Error::InvalidRequest(e.to_string()))?;
        let (status, res_body) = tinynet::send_request(req).await?;

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
        let body = serialize_request(&request)?;

        let base = self.base_url.trim_end_matches('/');
        let path = if base.ends_with("/v1") {
            "/chat/completions"
        } else {
            "/v1/chat/completions"
        };

        let req = Request::builder()
            .method("POST")
            .uri(format!("{}{}", base, path))
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", token))
            .body(Full::new(Bytes::from(body)))
            .map_err(|e| Error::InvalidRequest(e.to_string()))?;
        let (status, res_body) = tinynet::send_request(req).await?;

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
        let req_body = match serialize_request(&request) {
            Ok(v) => v,
            Err(err) => {
                return Box::pin(futures::stream::once(async move { Err(err) }));
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
            .uri(format!("{}{}", base, path))
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
            let mut upstream = tinynet::stream_request(req).await;
            let mut decoder = tinynet::SseDecoder::new();
            let mut parsed = Vec::new();

            while let Some(item) = upstream.next().await {
                let chunk = match item {
                    Ok(data) => data,
                    Err(e) => {
                        yield Err(e.into());
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
