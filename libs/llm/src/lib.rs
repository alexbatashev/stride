mod anthropic;
mod completion_request;
mod mock;
mod ollama;
mod openai;
mod utils;

use futures::Stream;
use serde::{Deserialize, Serialize};
use std::pin::Pin;
use thiserror::Error;

pub use anthropic::Anthropic;
pub use completion_request::{
    CompletionRequest, Function, FunctionParameters, FunctionProperty, Tool, ToolChoice, ToolType,
    UnnamedToolChoice,
};
pub use mock::Mock;
pub use ollama::Ollama;
pub use openai::OpenAI;
pub use utils::SharedExecutor;

#[derive(Debug, Clone)]
pub enum API {
    OpenAI(OpenAI),
    Anthropic(Anthropic),
    Ollama(Ollama),
    Mock(Mock),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Role {
    #[serde(rename = "system")]
    System,
    #[serde(rename = "assistant")]
    Assistant,
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
    #[serde(skip_serializing_if = "Option::is_none")]
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
pub struct ModelDesc {
    pub id: String,
    pub object: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owned_by: Option<String>,
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
    #[error("TLS Client error: {0}")]
    TlsError(String),
    #[error("Invalid request: {0}")]
    InvalidRequest(String),
    #[error("Unknown error")]
    Unknown,
}

impl API {
    pub async fn list_models(&self, token: &str) -> Result<Vec<ModelDesc>, Error> {
        match self {
            API::OpenAI(api) => api.list_models(token).await,
            API::Anthropic(api) => api.list_models(token).await,
            API::Ollama(api) => api.list_models(token).await,
            API::Mock(api) => api.list_models(token).await,
        }
    }

    pub async fn get_model(&self, token: &str, model_name: &str) -> Result<ModelDesc, Error> {
        match self {
            API::OpenAI(api) => api.get_model(token, model_name).await,
            API::Anthropic(api) => api.get_model(token, model_name).await,
            API::Ollama(api) => api.get_model(token, model_name).await,
            API::Mock(api) => api.get_model(token, model_name).await,
        }
    }

    pub async fn get_embeddings(
        &self,
        token: &str,
        input: &str,
        model: &str,
    ) -> Result<EmbeddingResponse, Error> {
        match self {
            API::OpenAI(api) => api.get_embeddings(token, input, model).await,
            API::Ollama(api) => api.get_embeddings(token, input, model).await,
            _ => unimplemented!(),
        }
    }

    pub async fn get_completion(
        &self,
        token: &str,
        request: CompletionRequest,
    ) -> Result<Completion, Error> {
        if request.stream.unwrap_or_default() {
            return Err(Error::InvalidRequest("expected stream == false".to_owned()));
        }
        match self {
            API::OpenAI(api) => api.get_completion(token, request).await,
            API::Anthropic(api) => api.get_completion(token, request).await,
            API::Ollama(api) => api.get_completion(token, request).await,
            API::Mock(api) => api.get_completion(token, request).await,
        }
    }

    pub fn stream_completion(
        &self,
        token: &str,
        request: CompletionRequest,
    ) -> Pin<Box<dyn Stream<Item = Result<StreamResponseChunk, Error>> + Send + 'static>> {
        let request = request.stream();
        match self {
            API::OpenAI(api) => api.stream_completion(token, request),
            API::Anthropic(api) => api.stream_completion(token, request),
            API::Ollama(api) => api.stream_completion(token, request),
            API::Mock(api) => api.stream_completion(token, request),
        }
    }
}

impl Into<axum::Error> for Error {
    fn into(self) -> axum::Error {
        axum::Error::new(self)
    }
}
