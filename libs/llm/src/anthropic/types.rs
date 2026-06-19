use serde::{Deserialize, Serialize};

use crate::{
    Completion, CompletionChoice, CompletionRequest, Message, ModelDesc, StreamResponseChunk, Usage,
};

#[derive(Debug, Deserialize)]
pub struct AnthropicModelList {
    pub models: Vec<ModelDesc>,
}

#[derive(Deserialize)]
pub struct AnthropicTextContent {
    pub r#type: String,
    pub text: String,
}

#[derive(Deserialize)]
pub struct AnthropicCompletionResponse {
    pub id: String,
    pub r#type: String,
    pub role: String,
    pub model: String,
    pub content: Vec<AnthropicTextContent>,
    pub stop_reason: Option<String>,
    pub stop_sequence: Option<String>,
    pub usage: AnthropicUsage,
}

#[derive(Deserialize)]
pub struct AnthropicUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

#[derive(Serialize)]
pub struct AnthropicThinking {
    r#type: &'static str,
    budget_tokens: u32,
}

#[derive(Serialize)]
pub struct AnthropicCompletionRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    messages: &'a [Message],
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<AnthropicThinking>,
}

#[derive(Deserialize)]
pub struct AnthropicStreamChunk {
    pub index: Option<u32>,
    pub delta: Option<AnthropicTextContent>,
    pub content_block: Option<AnthropicTextContent>,
    pub text: Option<String>,
}

pub struct AnthropicStreamChunkWithModel {
    pub model: String,
    pub chunk: AnthropicStreamChunk,
}

/// Headroom for output tokens added on top of the thinking budget, since
/// Anthropic requires `max_tokens` to be strictly greater than `budget_tokens`.
const THINKING_OUTPUT_HEADROOM: u32 = 8192;

impl<'a> AnthropicCompletionRequest<'a> {
    pub fn stream_with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = self.clamp_max_tokens(max_tokens);
        self.stream = Some(true);
        self
    }

    fn clamp_max_tokens(&self, max_tokens: u32) -> u32 {
        match &self.thinking {
            Some(thinking) => max_tokens.max(thinking.budget_tokens + THINKING_OUTPUT_HEADROOM),
            None => max_tokens,
        }
    }
}

impl<'a> From<&'a CompletionRequest> for AnthropicCompletionRequest<'a> {
    fn from(value: &'a CompletionRequest) -> Self {
        let thinking = value.reasoning_effort.map(|effort| AnthropicThinking {
            r#type: "enabled",
            budget_tokens: effort.budget_tokens(),
        });
        let mut request = Self {
            model: &value.model,
            max_tokens: value.max_tokens.unwrap_or(8192),
            messages: &value.messages,
            system: None,
            stream: None,
            thinking,
        };
        request.max_tokens = request.clamp_max_tokens(request.max_tokens);
        request
    }
}

impl From<AnthropicCompletionResponse> for Completion {
    fn from(value: AnthropicCompletionResponse) -> Self {
        let choices = value
            .content
            .into_iter()
            .enumerate()
            .map(|(i, content)| CompletionChoice {
                message: None,
                text: Some(content.text),
                index: i as u16,
                delta: None,
                logprobs: None,
                tool_calls: None,
                finish_reason: value.stop_reason.clone(),
            })
            .collect();

        Self {
            id: value.id,
            created: 0,
            model: value.model,
            choices,
            usage: Usage {
                prompt_tokens: value.usage.input_tokens,
                completion_tokens: value.usage.output_tokens,
                total_tokens: value.usage.input_tokens + value.usage.output_tokens,
            },
        }
    }
}

impl TryFrom<AnthropicStreamChunkWithModel> for StreamResponseChunk {
    type Error = ();

    fn try_from(value: AnthropicStreamChunkWithModel) -> Result<Self, Self::Error> {
        let text = value
            .chunk
            .text
            .or_else(|| value.chunk.delta.as_ref().map(|delta| delta.text.clone()))
            .or_else(|| {
                value
                    .chunk
                    .content_block
                    .as_ref()
                    .map(|content| content.text.clone())
            })
            .ok_or(())?;

        Ok(Self {
            id: format!("cmpl-{}", uuid::Uuid::new_v4()),
            object: "chat.completion.chunk".to_string(),
            created: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            model: value.model,
            system_fingerprint: None,
            choices: vec![CompletionChoice {
                message: None,
                text: Some(text),
                index: value.chunk.index.unwrap_or(0) as u16,
                delta: None,
                logprobs: None,
                tool_calls: None,
                finish_reason: None,
            }],
        })
    }
}
