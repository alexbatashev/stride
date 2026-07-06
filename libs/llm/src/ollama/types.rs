use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::{
    Completion, CompletionChoice, CompletionRequest, Delta, EmbeddingData, EmbeddingResponse,
    ImageSource, Message, ModelDesc, ReasoningEffort, Role, StreamResponseChunk, Tool,
    ToolCallChunk, ToolCallFunction, Usage,
};
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize)]
pub struct OllamaModel {
    model: Option<String>,
    name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OllamaChatRequest<'a> {
    model: &'a str,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    think: Option<OllamaThink>,
    messages: Vec<OllamaRequestMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<&'a [Tool]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<OllamaOptions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    logprobs: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_logprobs: Option<u8>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
enum OllamaThink {
    Level(&'static str),
}

#[derive(Debug, Clone, Default, Serialize)]
struct OllamaOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    num_predict: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stop: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    frequency_penalty: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    presence_penalty: Option<f32>,
}

#[derive(Debug, Clone, Serialize)]
struct OllamaRequestMessage {
    role: Role,
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    images: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OllamaRequestToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct OllamaRequestToolCall {
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    call_type: Option<String>,
    function: OllamaRequestToolCallFunction,
}

#[derive(Debug, Clone, Serialize)]
struct OllamaRequestToolCallFunction {
    #[serde(skip_serializing_if = "Option::is_none")]
    index: Option<usize>,
    name: String,
    arguments: serde_json::Value,
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
    pub message: OllamaResponseMessage,
    pub done: bool,
    pub done_reason: Option<String>,
    pub prompt_eval_count: Option<u32>,
    pub eval_count: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OllamaResponseMessage {
    pub role: Role,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub thinking: Option<String>,
    #[serde(default)]
    pub tool_calls: Option<Vec<OllamaResponseToolCall>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OllamaResponseToolCall {
    #[serde(rename = "type")]
    pub call_type: Option<String>,
    pub function: OllamaResponseToolCallFunction,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OllamaResponseToolCallFunction {
    pub index: Option<usize>,
    pub name: Option<String>,
    #[serde(default)]
    pub arguments: serde_json::Value,
}

impl From<OllamaModel> for ModelDesc {
    fn from(val: OllamaModel) -> ModelDesc {
        ModelDesc {
            id: val.model.or(val.name).unwrap_or_default(),
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
            think: value
                .reasoning_effort
                .map(|effort| ollama_think(&value.model, effort)),
            messages: ollama_messages(&value.messages),
            tools: value.tools.as_deref(),
            options: ollama_options(value),
            logprobs: value.logprobs,
            top_logprobs: value.top_logprobs,
        }
    }
}

impl From<OllamaMessageResponse> for Completion {
    fn from(val: OllamaMessageResponse) -> Completion {
        let prompt_tokens = val.prompt_eval_count.unwrap_or_default();
        let completion_tokens = val.eval_count.unwrap_or_default();
        let finish_reason = val.done_reason;
        let message = val.message.into_message();
        Completion {
            id: Uuid::now_v7().into(),
            created: 0,
            model: val.model,
            choices: vec![CompletionChoice {
                message: Some(message),
                text: None,
                index: 0,
                delta: None,
                logprobs: None,
                tool_calls: None,
                finish_reason,
            }],
            usage: Usage {
                prompt_tokens,
                completion_tokens,
                total_tokens: prompt_tokens + completion_tokens,
            },
        }
    }
}

impl From<OllamaMessageResponse> for StreamResponseChunk {
    fn from(val: OllamaMessageResponse) -> StreamResponseChunk {
        let usage = match (val.prompt_eval_count, val.eval_count) {
            (None, None) => None,
            (prompt, completion) => {
                let prompt_tokens = prompt.unwrap_or_default();
                let completion_tokens = completion.unwrap_or_default();
                Some(Usage {
                    prompt_tokens,
                    completion_tokens,
                    total_tokens: prompt_tokens + completion_tokens,
                })
            }
        };
        let message = val.message.into_message();
        StreamResponseChunk {
            id: Uuid::now_v7().into(),
            object: "completion".to_string(),
            created: 0,
            model: val.model.clone(),
            system_fingerprint: None,
            usage,
            choices: vec![CompletionChoice {
                message: Some(message.clone()),
                text: None,
                index: 0,
                delta: Some(Delta {
                    content: (!message.content.is_empty()).then_some(message.content.clone()),
                    thinking: message.thinking.clone(),
                    tool_calls: message.tool_calls.clone(),
                }),
                logprobs: None,
                tool_calls: None,
                finish_reason: val.done_reason.clone(),
            }],
        }
    }
}

impl From<OllamaEmbeddingsResponse> for EmbeddingResponse {
    fn from(mut val: OllamaEmbeddingsResponse) -> EmbeddingResponse {
        EmbeddingResponse {
            object: "object".to_string(),
            model: val.model,
            data: vec![EmbeddingData {
                object: "list".to_string(),
                index: 0,
                embedding: std::mem::take(&mut val.embeddings[0]),
            }],
            usage: Usage {
                prompt_tokens: val.prompt_eval_count,
                completion_tokens: 0,
                total_tokens: val.prompt_eval_count,
            },
        }
    }
}

fn ollama_think(model: &str, effort: ReasoningEffort) -> OllamaThink {
    match effort {
        ReasoningEffort::Low => OllamaThink::Level("low"),
        ReasoningEffort::Medium => OllamaThink::Level("medium"),
        ReasoningEffort::High => OllamaThink::Level("high"),
        ReasoningEffort::Xhigh if model.starts_with("gpt-oss") => OllamaThink::Level("high"),
        ReasoningEffort::Xhigh => OllamaThink::Level("max"),
    }
}

fn ollama_options(request: &CompletionRequest) -> Option<OllamaOptions> {
    let options = OllamaOptions {
        temperature: request.temperature,
        top_p: request.top_p,
        num_predict: request.max_tokens,
        stop: request.stop.clone(),
        frequency_penalty: request.frequency_penalty,
        presence_penalty: request.presence_penalty,
    };
    (options.temperature.is_some()
        || options.top_p.is_some()
        || options.num_predict.is_some()
        || options.stop.is_some()
        || options.frequency_penalty.is_some()
        || options.presence_penalty.is_some())
    .then_some(options)
}

fn ollama_messages(messages: &[Message]) -> Vec<OllamaRequestMessage> {
    let mut tool_names_by_id = HashMap::new();
    let mut result = Vec::with_capacity(messages.len());

    for message in messages {
        let tool_calls = message.tool_calls.as_ref().map(|calls| {
            calls
                .iter()
                .enumerate()
                .filter_map(|(fallback_index, call)| {
                    ollama_request_tool_call(call, fallback_index, &mut tool_names_by_id)
                })
                .collect::<Vec<_>>()
        });

        result.push(OllamaRequestMessage {
            role: message.role.clone(),
            content: message.content.clone(),
            images: message.images.as_deref().map(ollama_images),
            thinking: message.thinking.clone(),
            tool_calls: tool_calls.filter(|calls| !calls.is_empty()),
            tool_name: message
                .tool_call_id
                .as_ref()
                .and_then(|id| tool_names_by_id.get(id).cloned())
                .or_else(|| message.tool_call_id.clone()),
        });
    }

    result
}

fn ollama_request_tool_call(
    call: &ToolCallChunk,
    fallback_index: usize,
    tool_names_by_id: &mut HashMap<String, String>,
) -> Option<OllamaRequestToolCall> {
    let function = call.function.as_ref()?;
    let name = function.name.clone()?;
    if let Some(id) = &call.id {
        tool_names_by_id.insert(id.clone(), name.clone());
    }
    Some(OllamaRequestToolCall {
        call_type: call
            .call_type
            .clone()
            .or_else(|| Some("function".to_string())),
        function: OllamaRequestToolCallFunction {
            index: call.index.or(Some(fallback_index)),
            name,
            arguments: function
                .arguments
                .as_deref()
                .and_then(|arguments| serde_json::from_str(arguments).ok())
                .unwrap_or(serde_json::Value::Object(Default::default())),
        },
    })
}

fn ollama_images(images: &[ImageSource]) -> Vec<String> {
    images
        .iter()
        .filter_map(|image| {
            image.data.clone().or_else(|| {
                image
                    .url
                    .as_deref()
                    .and_then(|url| url.split_once(";base64,").map(|(_, data)| data.to_string()))
            })
        })
        .collect()
}

impl OllamaResponseMessage {
    fn into_message(self) -> Message {
        Message {
            role: self.role,
            content: self.content,
            images: None,
            thinking: self.thinking,
            tool_calls: self.tool_calls.map(|calls| {
                calls
                    .into_iter()
                    .enumerate()
                    .map(|(index, call)| call.into_tool_call(index))
                    .collect()
            }),
            tool_call_id: None,
        }
    }
}

impl OllamaResponseToolCall {
    fn into_tool_call(self, fallback_index: usize) -> ToolCallChunk {
        let index = self.function.index.unwrap_or(fallback_index);
        ToolCallChunk {
            index: Some(index),
            id: Some(format!("call_{}", Uuid::now_v7().simple())),
            call_type: self.call_type.or_else(|| Some("function".to_string())),
            function: Some(ToolCallFunction {
                name: self.function.name,
                arguments: Some(match self.function.arguments {
                    serde_json::Value::String(arguments) => arguments,
                    arguments => arguments.to_string(),
                }),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        CompletionRequest, Function, ImageSource, Message, ReasoningEffort, Role, ToolCallChunk,
        ToolCallFunction,
    };

    #[test]
    fn serializes_cloud_capabilities_for_ollama_chat() {
        let request = CompletionRequest {
            model: "gpt-oss:120b".to_string(),
            messages: vec![Message {
                role: Role::User,
                content: "look".to_string(),
                images: Some(vec![ImageSource::base64("image/png", "abc")]),
                ..Default::default()
            }],
            reasoning_effort: Some(ReasoningEffort::Xhigh),
            temperature: Some(0.2),
            max_tokens: Some(128),
            tools: Some(vec![
                Function {
                    description: "Read a file".to_string(),
                    name: "read_file".to_string(),
                    parameters: None,
                }
                .into(),
            ]),
            ..Default::default()
        };

        let body = serde_json::to_value(super::OllamaChatRequest::from(&request)).unwrap();

        assert_eq!(body["think"], "high");
        assert_eq!(body["options"]["temperature"].as_f64().unwrap() as f32, 0.2);
        assert_eq!(body["options"]["num_predict"], 128);
        assert_eq!(body["messages"][0]["images"][0], "abc");
        assert_eq!(body["tools"][0]["function"]["name"], "read_file");
    }

    #[test]
    fn parses_tool_call_arguments_as_json_string() {
        let body = br#"{
            "model": "qwen3",
            "message": {
                "role": "assistant",
                "content": "",
                "tool_calls": [{
                    "function": {
                        "index": 0,
                        "name": "read_file",
                        "arguments": {"path": "Cargo.toml"}
                    }
                }]
            },
            "done": true
        }"#;

        let response: super::OllamaMessageResponse = serde_json::from_slice(body).unwrap();
        let message = response.message.into_message();
        let mut calls = message.tool_calls.unwrap();
        let call = calls.remove(0);

        assert_eq!(
            call.function.unwrap().arguments.unwrap(),
            r#"{"path":"Cargo.toml"}"#
        );
    }

    #[test]
    fn serializes_tool_results_with_ollama_tool_name() {
        let messages = vec![
            Message {
                role: Role::Assistant,
                content: String::new(),
                tool_calls: Some(vec![ToolCallChunk {
                    index: Some(0),
                    id: Some("call_1".to_string()),
                    call_type: Some("function".to_string()),
                    function: Some(ToolCallFunction {
                        name: Some("read_file".to_string()),
                        arguments: Some(r#"{"path":"Cargo.toml"}"#.to_string()),
                    }),
                }]),
                ..Default::default()
            },
            Message {
                role: Role::Tool,
                content: "contents".to_string(),
                tool_call_id: Some("call_1".to_string()),
                ..Default::default()
            },
        ];
        let request = CompletionRequest {
            model: "qwen3".to_string(),
            messages,
            ..Default::default()
        };

        let body = serde_json::to_value(super::OllamaChatRequest::from(&request)).unwrap();

        assert_eq!(body["messages"][1]["tool_name"], "read_file");
        assert_eq!(
            body["messages"][0]["tool_calls"][0]["function"]["arguments"]["path"],
            "Cargo.toml"
        );
    }
}
