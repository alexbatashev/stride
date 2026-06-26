use crate::{
    Completion, CompletionRequest, EmbeddingResponse, Error, ImageSource, ModelDesc,
    StreamResponseChunk, Transcription,
};

use async_stream::stream;
use bytes::Bytes;
use futures::{Stream, StreamExt};
use http_body_util::Full;
use hyper::Request;
use serde::{Deserialize, Serialize};
use std::pin::Pin;

/// How the reasoning effort is serialized on the wire. OpenAI's chat
/// completions API takes a flat `reasoning_effort` string; OpenRouter expects a
/// nested `reasoning: { effort }` object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReasoningStyle {
    Effort,
    Nested,
}

#[derive(Clone)]
pub struct OpenAI {
    base_url: String,
    reasoning_style: ReasoningStyle,
}

impl std::fmt::Debug for OpenAI {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenAI")
            .field("base_url", &self.base_url)
            .field("reasoning_style", &self.reasoning_style)
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

        let body = serialize_request(&request, ReasoningStyle::Effort).unwrap();
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

        let body = serialize_request(&request, ReasoningStyle::Effort).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["messages"][0]["content"], "hello");
    }

    #[test]
    fn serializes_reasoning_effort_as_string() {
        use crate::ReasoningEffort;

        let request = CompletionRequest {
            model: "o4".to_string(),
            reasoning_effort: Some(ReasoningEffort::High),
            ..Default::default()
        };

        let body = serialize_request(&request, ReasoningStyle::Effort).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["reasoning_effort"], "high");
        assert!(value.get("reasoning").is_none());
    }

    #[test]
    fn serializes_reasoning_as_nested_object_for_openrouter() {
        use crate::ReasoningEffort;

        let request = CompletionRequest {
            model: "anthropic/claude".to_string(),
            reasoning_effort: Some(ReasoningEffort::Xhigh),
            ..Default::default()
        };

        let body = serialize_request(&request, ReasoningStyle::Nested).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["reasoning"]["effort"], "xhigh");
        assert!(value.get("reasoning_effort").is_none());
    }

    #[test]
    fn builds_multipart_transcription_body() {
        let body = build_transcription_body(
            "BOUND",
            b"audiobytes",
            "voice.ogg",
            "audio/ogg",
            "whisper-1",
        );
        let text = String::from_utf8_lossy(&body);

        assert!(text.contains("--BOUND\r\n"));
        assert!(text.contains("name=\"model\""));
        assert!(text.contains("whisper-1"));
        assert!(text.contains("name=\"response_format\""));
        assert!(text.contains("name=\"file\"; filename=\"voice.ogg\""));
        assert!(text.contains("Content-Type: audio/ogg"));
        assert!(text.contains("audiobytes"));
        assert!(text.ends_with("--BOUND--\r\n"));
    }

    #[test]
    fn maps_audio_mime_to_openrouter_format() {
        assert_eq!(audio_format_from_mime("audio/ogg"), "ogg");
        assert_eq!(audio_format_from_mime("audio/webm;codecs=opus"), "webm");
        assert_eq!(audio_format_from_mime("audio/mpeg"), "mp3");
        assert_eq!(audio_format_from_mime("audio/x-wav"), "wav");
        assert_eq!(
            audio_format_from_mime("application/octet-stream"),
            "octet-stream"
        );
    }

    #[test]
    fn openrouter_transcription_uses_json_base64_body() {
        let client = OpenAI::openrouter("https://openrouter.ai/api/v1");
        let req = client
            .transcription_json_request("tok", b"audiobytes", "audio/ogg", "openai/whisper-1")
            .unwrap();
        assert_eq!(
            req.headers().get("Content-Type").unwrap(),
            "application/json"
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

/// Serializes a request to OpenAI's wire format, rewriting any message that
/// carries images into the `content` parts array (text part plus one
/// `image_url` part per image) that the chat completions API expects.
fn serialize_request(
    request: &CompletionRequest,
    reasoning_style: ReasoningStyle,
) -> Result<Vec<u8>, Error> {
    let mut body =
        serde_json::to_value(request).map_err(|e| Error::ParsingError(format!("{:?}", e)))?;

    if let Some(messages) = body.get_mut("messages").and_then(|m| m.as_array_mut()) {
        for message in messages {
            rewrite_message_images(message);
        }
    }

    if reasoning_style == ReasoningStyle::Nested {
        rewrite_reasoning_effort(&mut body);
    }

    serde_json::to_vec(&body).map_err(|e| Error::ParsingError(format!("{:?}", e)))
}

/// Builds a `multipart/form-data` body carrying the audio file plus the `model`
/// and `response_format` fields the transcription endpoint expects.
fn build_transcription_body(
    boundary: &str,
    audio: &[u8],
    file_name: &str,
    mime_type: &str,
    model: &str,
) -> Vec<u8> {
    let mut body = Vec::new();
    let mut text_field = |name: &str, value: &str| {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        body.extend_from_slice(
            format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
        );
        body.extend_from_slice(value.as_bytes());
        body.extend_from_slice(b"\r\n");
    };
    text_field("model", model);
    text_field("response_format", "json");

    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        format!("Content-Disposition: form-data; name=\"file\"; filename=\"{file_name}\"\r\n")
            .as_bytes(),
    );
    body.extend_from_slice(format!("Content-Type: {mime_type}\r\n\r\n").as_bytes());
    body.extend_from_slice(audio);
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    body
}

/// Maps an audio MIME type to the short format token OpenRouter expects
/// (e.g. `audio/ogg` -> `ogg`), defaulting to the subtype after the slash.
fn audio_format_from_mime(mime_type: &str) -> &str {
    let subtype = mime_type
        .split(';')
        .next()
        .unwrap_or(mime_type)
        .trim()
        .rsplit('/')
        .next()
        .unwrap_or("")
        .trim();
    match subtype {
        "mpeg" | "mp3" => "mp3",
        "x-wav" | "wave" | "wav" => "wav",
        "" => "webm",
        other => other,
    }
}

/// Rewrites the flat `reasoning_effort` string into OpenRouter's nested
/// `reasoning: { effort }` object.
fn rewrite_reasoning_effort(body: &mut serde_json::Value) {
    let Some(object) = body.as_object_mut() else {
        return;
    };
    let Some(effort) = object.remove("reasoning_effort") else {
        return;
    };
    object.insert(
        "reasoning".to_string(),
        serde_json::json!({ "effort": effort }),
    );
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
            reasoning_style: ReasoningStyle::Effort,
        }
    }

    /// Builds a client for OpenRouter, which expects reasoning effort as a
    /// nested `reasoning: { effort }` object instead of a flat string.
    pub fn openrouter(base_url: &str) -> Self {
        OpenAI {
            base_url: base_url.to_string(),
            reasoning_style: ReasoningStyle::Nested,
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

    /// Transcribes audio via the `/v1/audio/transcriptions` endpoint. OpenAI and
    /// other Whisper-compatible providers take a `multipart/form-data` upload;
    /// OpenRouter instead wants a JSON body carrying base64-encoded audio, so the
    /// request shape follows the same flavor used for reasoning effort.
    pub async fn transcribe(
        &self,
        token: &str,
        audio: &[u8],
        file_name: &str,
        mime_type: &str,
        model: &str,
    ) -> Result<Transcription, Error> {
        let req = match self.reasoning_style {
            ReasoningStyle::Nested => {
                self.transcription_json_request(token, audio, mime_type, model)
            }
            ReasoningStyle::Effort => {
                self.transcription_multipart_request(token, audio, file_name, mime_type, model)
            }
        }?;

        let (status, res_body) = tinynet::send_request(req).await?;

        if !(200..300).contains(&status) {
            let message = String::from_utf8_lossy(&res_body).trim().to_string();
            return Err(Error::ServerErrorWithMessage(status, message));
        }

        serde_json::from_slice(&res_body).map_err(|e| Error::ParsingError(format!("{:?}", e)))
    }

    /// OpenAI-style `multipart/form-data` transcription request.
    fn transcription_multipart_request(
        &self,
        token: &str,
        audio: &[u8],
        file_name: &str,
        mime_type: &str,
        model: &str,
    ) -> Result<Request<Full<Bytes>>, Error> {
        let boundary = format!("----stride{}", uuid::Uuid::new_v4().simple());
        let body = build_transcription_body(&boundary, audio, file_name, mime_type, model);

        Request::builder()
            .method("POST")
            .uri(self.endpoint("/v1/audio/transcriptions"))
            .header(
                "Content-Type",
                format!("multipart/form-data; boundary={boundary}"),
            )
            .header("Authorization", format!("Bearer {}", token))
            .body(Full::new(Bytes::from(body)))
            .map_err(|e| Error::InvalidRequest(e.to_string()))
    }

    /// OpenRouter-style JSON transcription request with base64-encoded audio.
    fn transcription_json_request(
        &self,
        token: &str,
        audio: &[u8],
        mime_type: &str,
        model: &str,
    ) -> Result<Request<Full<Bytes>>, Error> {
        use base64::Engine;

        let data = base64::engine::general_purpose::STANDARD.encode(audio);
        let body = serde_json::to_vec(&serde_json::json!({
            "model": model,
            "input_audio": {
                "data": data,
                "format": audio_format_from_mime(mime_type),
            },
        }))
        .map_err(|e| Error::ParsingError(format!("{:?}", e)))?;

        Request::builder()
            .method("POST")
            .uri(self.endpoint("/v1/audio/transcriptions"))
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", token))
            .body(Full::new(Bytes::from(body)))
            .map_err(|e| Error::InvalidRequest(e.to_string()))
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
        let body = serialize_request(&request, self.reasoning_style)?;

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
        let req_body = match serialize_request(&request, self.reasoning_style) {
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
