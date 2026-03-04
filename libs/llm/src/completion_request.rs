use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::Message;

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

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub enum ToolType {
    #[default]
    #[serde(rename = "function")]
    Function,
}

#[derive(Clone, Default, Serialize, Debug, Deserialize)]
pub struct Tool {
    pub r#type: ToolType,
    pub function: Function,
}

#[derive(Clone, Default, Serialize, Debug, Deserialize)]
pub struct Function {
    pub description: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<Vec<FunctionParameters>>,
}

#[derive(Clone, Default, Serialize, Debug, Deserialize)]
pub struct FunctionParameters {
    pub r#type: String,
    pub properties: HashMap<String, FunctionProperty>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<Vec<String>>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct FunctionProperty {
    pub r#type: String,
    pub description: String,
}

#[derive(Clone, Default, Serialize, Deserialize, Debug)]
pub struct CompletionRequest {
    pub model: String,
    pub messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolChoice {
    Unnamed(UnnamedToolChoice),
    Named {
        r#type: String,
        function: FunctionRef,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UnnamedToolChoice {
    None,
    Auto,
    Required,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionRef {
    name: String,
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
}

impl Into<Tool> for Function {
    fn into(self) -> Tool {
        Tool {
            r#type: ToolType::Function,
            function: self,
        }
    }
}

impl Into<ToolChoice> for UnnamedToolChoice {
    fn into(self) -> ToolChoice {
        ToolChoice::Unnamed(self)
    }
}

impl Into<ToolChoice> for FunctionRef {
    fn into(self) -> ToolChoice {
        ToolChoice::Named {
            r#type: "function".to_string(),
            function: self,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{Message, Role, completion_request::UnnamedToolChoice};

    use super::{CompletionRequest, Function};

    #[test]
    fn tools() {
        let request = CompletionRequest::new(
            "test-model",
            &[Message {
                role: Role::User,
                content: "Hello".to_string(),
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

        insta::assert_snapshot!(json);
    }
}
