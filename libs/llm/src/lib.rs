mod anthropic;
mod api;
mod mock;
mod ollama;
mod openai;
mod types;
mod utils;

use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

pub use api::API;
pub use types::*;

pub use anthropic::Anthropic;
pub use mock::Mock;
pub use ollama::Ollama;
pub use openai::OpenAI;
pub use utils::*;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Message {
    pub role: Role,
    #[serde(default, deserialize_with = "deserialize_null_string_default")]
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCallChunk>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

fn deserialize_null_string_default<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<String>::deserialize(deserializer)?.unwrap_or_default())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Role {
    #[serde(rename = "system")]
    System,
    #[serde(rename = "assistant")]
    Assistant,
    #[default]
    #[serde(rename = "user")]
    User,
    #[serde(rename = "tool")]
    Tool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Completion {
    pub id: String,
    pub created: u32,
    pub model: String,
    pub choices: Vec<CompletionChoice>,
    pub usage: Usage,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StreamResponseChunk {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub system_fingerprint: Option<String>,
    pub choices: Vec<CompletionChoice>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Delta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "reasoning")]
    pub thinking: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCallChunk>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallChunk {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub call_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function: Option<ToolCallFunction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallFunction {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CompletionChoice {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    pub index: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta: Option<Delta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logprobs: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCallChunk>>,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EmbeddingData {
    pub object: String,
    pub index: u32,
    pub embedding: Vec<f32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EmbeddingResponse {
    pub object: String,
    pub model: String,
    pub data: EmbeddingData,
    pub usage: Usage,
}

#[derive(Debug, Error, Serialize, Deserialize)]
pub enum Error {
    #[error("Remote request error: {0}")]
    RequestError(String),
    #[error("Failed to parse response: {0}")]
    ParsingError(String),
    #[error("Internal server error: {0}")]
    ServerError(u16),
    #[error("Server error {0}: {1}")]
    ServerErrorWithMessage(u16, String),
    #[error("TLS Client error: {0}")]
    TlsError(String),
    #[error("Invalid request: {0}")]
    InvalidRequest(String),
    #[error("Unknown error")]
    Unknown,
}

impl From<Error> for axum::Error {
    fn from(val: Error) -> axum::Error {
        axum::Error::new(val)
    }
}

impl From<tinynet::Error> for Error {
    fn from(e: tinynet::Error) -> Self {
        match e {
            tinynet::Error::RequestError(s) => Self::RequestError(s),
            tinynet::Error::TlsError(s) => Self::TlsError(s),
            tinynet::Error::InvalidRequest(s) => Self::InvalidRequest(s),
            tinynet::Error::ServerError(code, msg) => Self::ServerErrorWithMessage(code, msg),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_message_content_allows_null() {
        let body = br#"{
            "id": "chatcmpl-test",
            "created": 0,
            "model": "test-model",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": null},
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
        }"#;

        let parsed: Completion = serde_json::from_slice(body).unwrap();
        let message = parsed.choices.into_iter().next().unwrap().message.unwrap();
        assert_eq!(message.content, "");
    }
}
