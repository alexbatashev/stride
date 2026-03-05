use serde::{Deserialize, Serialize};

use crate::{
    Completion, CompletionChoice, CompletionRequest, Delta, EmbeddingData, EmbeddingResponse,
    Message, ModelDesc, StreamResponseChunk, Usage,
};
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize)]
pub struct OllamaModel {
    model: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct OllamaChatRequest<'a> {
    model: &'a str,
    stream: bool,
    think: bool,
    messages: &'a [Message],
    tools: Option<Vec<()>>, // TODO proper tools support
}

#[derive(Deserialize)]
pub struct OllamaEmbeddingsResponse {
    model: String,
    embeddings: Vec<Vec<f32>>,
    prompt_eval_count: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OllamaMessageResponse {
    pub model: String,
    pub message: Message,
    pub done: bool,
    pub done_reason: Option<String>,
    pub prompt_eval_count: Option<u32>,
    pub eval_count: Option<u32>,
}

impl Into<ModelDesc> for OllamaModel {
    fn into(self) -> ModelDesc {
        ModelDesc {
            id: self.model,
            ..Default::default()
        }
    }
}

impl<'a> OllamaChatRequest<'a> {
    pub fn stream(mut self) -> Self {
        self.stream = true;
        self
    }
}

impl<'a> From<&'a CompletionRequest> for OllamaChatRequest<'a> {
    fn from(value: &'a CompletionRequest) -> Self {
        Self {
            model: &value.model,
            stream: false,
            think: false,
            messages: &value.messages,
            tools: None,
        }
    }
}

impl Into<Completion> for OllamaMessageResponse {
    fn into(self) -> Completion {
        let prompt_tokens = self.prompt_eval_count.unwrap_or_default();
        let completion_tokens = self.eval_count.unwrap_or_default();
        Completion {
            id: Uuid::now_v7().into(),
            created: 0,
            model: self.model,
            choices: vec![CompletionChoice {
                message: Some(self.message),
                text: None,
                index: 0,
                delta: None,
                logprobs: None,
                tool_calls: None,
                finish_reason: self.done_reason,
            }],
            usage: Usage {
                prompt_tokens,
                completion_tokens,
                total_tokens: prompt_tokens + completion_tokens,
            },
        }
    }
}

impl Into<StreamResponseChunk> for OllamaMessageResponse {
    fn into(self) -> StreamResponseChunk {
        StreamResponseChunk {
            id: Uuid::now_v7().into(),
            object: "completion".to_string(),
            created: 0,
            model: self.model.clone(),
            system_fingerprint: None,
            choices: vec![CompletionChoice {
                message: Some(self.message.clone()),
                text: None,
                index: 0,
                delta: Some(Delta {
                    content: Some(self.message.content.clone()),
                    thinking: self.message.thinking.clone(),
                    tool_calls: None,
                }),
                logprobs: None,
                tool_calls: None,
                finish_reason: self.done_reason.clone(),
            }],
        }
    }
}

impl Into<EmbeddingResponse> for OllamaEmbeddingsResponse {
    fn into(mut self) -> EmbeddingResponse {
        EmbeddingResponse {
            object: "object".to_string(),
            model: self.model,
            data: EmbeddingData {
                object: "list".to_string(),
                index: 0,
                embedding: std::mem::replace(&mut self.embeddings[0], vec![]),
            },
            usage: Usage {
                prompt_tokens: self.prompt_eval_count,
                completion_tokens: 0,
                total_tokens: self.prompt_eval_count,
            },
        }
    }
}
