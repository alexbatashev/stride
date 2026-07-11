use serde::{Deserialize, Serialize};

use crate::Message;

use super::{Function, FunctionRef, Tool, ToolChoice, ToolType, UnnamedToolChoice};

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub enum ResposeFormatType {
    #[default]
    #[serde(rename = "text")]
    Text,
    #[serde(rename = "json_object")]
    JsonObject,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ResponseFormat {
    r#type: ResposeFormatType,
}

#[derive(Clone, Default, Serialize, Deserialize, Debug)]
pub struct CompletionRequest {
    pub model: String,
    pub messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<ReasoningEffort>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<ResponseFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logprobs: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_logprobs: Option<u8>,
}

/// Reasoning effort level requested from a model. Serializes to the lowercase
/// string form (`"low"`, `"medium"`, `"high"`, `"xhigh"`) used by
/// OpenAI-compatible reasoning parameters.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningEffort {
    Low,
    Medium,
    High,
    Xhigh,
}

impl ReasoningEffort {
    /// Maps an effort level to a thinking token budget for providers that
    /// configure reasoning by token count instead of an effort string.
    pub fn budget_tokens(self) -> u32 {
        match self {
            ReasoningEffort::Low => 4096,
            ReasoningEffort::Medium => 8192,
            ReasoningEffort::High => 24576,
            ReasoningEffort::Xhigh => 32768,
        }
    }
}

impl CompletionRequest {
    pub fn new(model: &str, messages: &[Message]) -> Self {
        CompletionRequest {
            model: model.to_owned(),
            messages: messages.to_vec(),
            ..Default::default()
        }
    }

    pub fn stream(mut self) -> Self {
        self.stream = Some(true);
        self
    }

    pub fn last_message_contains(&self, needle: &str) -> bool {
        self.messages
            .last()
            .is_some_and(|message| message.content.contains(needle))
    }

    pub fn last_tool_result_contains(&self, needle: &str) -> bool {
        self.messages
            .iter()
            .rev()
            .find(|message| message.role == crate::Role::Tool)
            .is_some_and(|message| message.content.contains(needle))
    }

    pub fn frequency_penalty(mut self, penalty: f32) -> Self {
        self.frequency_penalty = Some(penalty);
        self
    }

    pub fn max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }

    pub fn presence_penalty(mut self, penalty: f32) -> Self {
        self.presence_penalty = Some(penalty);
        self
    }

    pub fn response_format(mut self, format: ResponseFormat) -> Self {
        self.response_format = Some(format);
        self
    }

    pub fn stop(mut self, stop: Vec<String>) -> Self {
        self.stop = Some(stop);
        self
    }

    pub fn temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }

    pub fn top_p(mut self, top_p: f32) -> Self {
        self.top_p = Some(top_p);
        self
    }

    pub fn tools(mut self, tools: Vec<Tool>) -> Self {
        self.tools = Some(tools);
        self
    }

    pub fn tool_choice<T: Into<ToolChoice>>(mut self, choice: T) -> Self {
        self.tool_choice = Some(choice.into());
        self
    }

    pub fn logprobs(mut self, logprobs: bool) -> Self {
        self.logprobs = Some(logprobs);
        self
    }

    pub fn top_logprobs(mut self, top_logprobs: u8) -> Self {
        self.top_logprobs = Some(top_logprobs);
        self
    }

    pub fn reasoning_effort(mut self, effort: ReasoningEffort) -> Self {
        self.reasoning_effort = Some(effort);
        self
    }
}

impl From<Function> for Tool {
    fn from(val: Function) -> Tool {
        Tool {
            r#type: ToolType::Function,
            function: val,
        }
    }
}

impl From<UnnamedToolChoice> for ToolChoice {
    fn from(val: UnnamedToolChoice) -> ToolChoice {
        ToolChoice::Unnamed(val)
    }
}

impl From<FunctionRef> for ToolChoice {
    fn from(val: FunctionRef) -> ToolChoice {
        ToolChoice::Named {
            r#type: "function".to_string(),
            function: val,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::env;

    use crate::{Message, Role, UnnamedToolChoice};

    use super::{CompletionRequest, Function};

    fn snapshot_path() -> String {
        if let Ok(path) = env::var("INSTA_SNAPSHOT_PATH") {
            return path;
        }

        if let Ok(runfiles_dir) = env::var("RUNFILES_DIR") {
            return format!("{runfiles_dir}/_main/libs/llm/src/types/snapshots");
        }

        "src/types/snapshots".to_string()
    }

    #[test]
    fn tools() {
        let request = CompletionRequest::new(
            "test-model",
            &[Message {
                role: Role::User,
                content: "Hello".to_string(),
                images: None,
                thinking: None,
                tool_calls: None,
                tool_call_id: None,
            }],
        )
        .frequency_penalty(1.0)
        .top_p(0.2)
        .tools(vec![
            Function {
                description: "Test function".to_string(),
                name: "test".to_string(),
                parameters: None,
            }
            .into(),
        ])
        .tool_choice(UnnamedToolChoice::Required);

        let json = serde_json::to_string(&request).unwrap();

        insta::with_settings!({snapshot_path => snapshot_path()}, {
            insta::assert_snapshot!(json);
        });
    }
}
