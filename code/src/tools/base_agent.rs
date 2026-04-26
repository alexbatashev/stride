use std::collections::HashMap;
use std::pin::Pin;

use anyhow::{Result, anyhow};
use futures::{Stream, StreamExt};
use llm::{
    API, CompletionRequest, Function, FunctionParameters, FunctionProperty, Message, Role,
    StreamResponseChunk, ThinkingConfig, ToolCallChunk, ToolCallFunction, ToolType,
    UnnamedToolChoice,
};
use serde_json::Value;

use super::{FunctionCall, Tool, ToolCall, ToolContext, ToolRegistry, ToolResult};

pub enum StreamEvent {
    Thinking(String),
    TextChunk(String),
}

#[derive(Default)]
struct PartialToolCall {
    id: String,
    name: String,
    arguments: String,
}

impl PartialToolCall {
    fn to_tool_call(self) -> ToolCall {
        ToolCall {
            id: self.id,
            call_type: "function".to_string(),
            function: FunctionCall {
                name: self.name,
                arguments: self.arguments,
            },
        }
    }
}

pub struct BaseAgent {
    pub(crate) api: API,
    pub(crate) token: String,
    pub(crate) model: String,
    pub(crate) conversation: Vec<Message>,
    pub(crate) tool_registry: ToolRegistry,
    pub(crate) tool_context: ToolContext,
    pub(crate) max_iterations: usize,
    pub(crate) thinking: Option<ThinkingConfig>,
    pub(crate) tool_display_names: HashMap<usize, String>,
}

impl BaseAgent {
    pub fn new(api: API, token: String, model: String, tool_context: ToolContext) -> Self {
        Self {
            api,
            token,
            model,
            conversation: Vec::new(),
            tool_registry: ToolRegistry::new(),
            tool_context,
            max_iterations: 10,
            thinking: None,
            tool_display_names: HashMap::new(),
        }
    }

    pub fn set_system_prompt(&mut self, prompt: String) {
        if let Some(msg) = self
            .conversation
            .iter_mut()
            .find(|m| m.role == Role::System)
        {
            msg.content = prompt;
        } else {
            self.conversation.insert(
                0,
                Message {
                    role: Role::System,
                    content: prompt,
                    thinking: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
            );
        }
    }

    pub fn register_tool(&mut self, tool: impl Tool + 'static) {
        self.tool_registry.register(tool);
    }

    pub fn set_max_iterations(&mut self, n: usize) {
        self.max_iterations = n;
    }

    pub fn set_thinking(&mut self, config: Option<ThinkingConfig>) {
        self.thinking = config;
    }

    pub fn set_conversation(&mut self, conversation: Vec<Message>) {
        self.conversation = conversation;
    }

    pub fn push_message(&mut self, message: Message) {
        self.conversation.push(message);
    }

    pub fn conversation(&self) -> &[Message] {
        &self.conversation
    }

    pub fn tool_display_names(&self) -> &HashMap<usize, String> {
        &self.tool_display_names
    }

    pub fn tool_registry(&self) -> &ToolRegistry {
        &self.tool_registry
    }

    pub async fn run(&mut self, user_prompt: String) -> Result<String> {
        self.conversation.push(Message {
            role: Role::User,
            content: user_prompt,
            thinking: None,
            tool_calls: None,
            tool_call_id: None,
        });

        for _ in 0..self.max_iterations {
            let (_events, tool_calls) = self.collect_response().await?;

            if tool_calls.is_empty() {
                return self.last_assistant_text();
            }

            for call in tool_calls {
                self.execute_tool(call).await?;
            }
        }

        Err(anyhow!(
            "BaseAgent reached maximum iteration limit ({})",
            self.max_iterations
        ))
    }

    pub async fn collect_response(&mut self) -> Result<(Vec<StreamEvent>, Vec<ToolCall>)> {
        let request = self.build_completion_request();
        let mut stream = self.api.stream_completion(&self.token, request);
        let assistant_idx = self.begin_assistant_message();
        self.collect_stream_inner(&mut stream, assistant_idx).await
    }

    pub async fn execute_tool(&mut self, call: ToolCall) -> Result<()> {
        let tool_name = call.function.name.clone();
        let args = call
            .parsed_arguments()
            .unwrap_or(Value::Object(serde_json::Map::new()));

        let result = if let Some(tool) = self.tool_registry.get(&tool_name) {
            tool.execute(args, &self.tool_context).await
        } else {
            ToolResult::error(format!("Unknown tool: {}", tool_name))
        };

        self.conversation.push(Message {
            role: Role::Tool,
            content: serde_json::to_string(&result)?,
            thinking: None,
            tool_calls: None,
            tool_call_id: Some(call.id),
        });
        self.tool_display_names
            .insert(self.conversation.len() - 1, tool_name);
        Ok(())
    }

    pub fn push_tool_error(&mut self, call_id: &str, msg: &str) {
        self.conversation.push(Message {
            role: Role::Tool,
            content: serde_json::to_string(&ToolResult::error(msg)).unwrap_or_default(),
            thinking: None,
            tool_calls: None,
            tool_call_id: Some(call_id.to_string()),
        });
        self.tool_display_names
            .insert(self.conversation.len() - 1, "tool error".to_string());
    }

    fn build_completion_request(&self) -> CompletionRequest {
        let llm_tools: Vec<llm::Tool> = self
            .tool_registry
            .definitions()
            .into_iter()
            .map(|def| {
                let params = FunctionParameters {
                    param_type: "object".to_string(),
                    properties: def
                        .function
                        .parameters
                        .properties
                        .into_iter()
                        .map(|(k, v)| {
                            (
                                k,
                                FunctionProperty {
                                    r#type: v.property_type,
                                    description: v.description,
                                },
                            )
                        })
                        .collect(),
                    required: Some(def.function.parameters.required),
                };

                llm::Tool {
                    r#type: ToolType::Function,
                    function: Function {
                        description: def.function.description,
                        name: def.function.name,
                        parameters: Some(params),
                    },
                }
            })
            .collect();

        let mut request = CompletionRequest::new(&self.model, &self.conversation).stream();

        if !llm_tools.is_empty() {
            request = request.tools(llm_tools);
            request = request.tool_choice(UnnamedToolChoice::Auto);
        }

        if let Some(ref thinking_config) = self.thinking {
            request = request.thinking(thinking_config.clone());
        }

        request
    }

    fn begin_assistant_message(&mut self) -> usize {
        self.conversation.push(Message {
            role: Role::Assistant,
            content: String::new(),
            thinking: None,
            tool_calls: None,
            tool_call_id: None,
        });
        self.conversation.len() - 1
    }

    async fn collect_stream_inner(
        &mut self,
        stream: &mut Pin<Box<dyn Stream<Item = Result<StreamResponseChunk, llm::Error>> + Send>>,
        assistant_idx: usize,
    ) -> Result<(Vec<StreamEvent>, Vec<ToolCall>)> {
        let mut thinking = String::new();
        let mut tool_calls: HashMap<usize, PartialToolCall> = HashMap::new();
        let mut events = Vec::new();
        let mut thinking_emitted = false;

        while let Some(item) = stream.next().await {
            let chunk = match item {
                Ok(c) => c,
                Err(e) => return Err(anyhow!("API error: {}", e)),
            };

            for choice in chunk.choices {
                if let Some(delta) = choice.delta {
                    if let Some(text) = delta.thinking {
                        thinking.push_str(&text);
                    }

                    if let Some(text) = delta.content {
                        if !thinking_emitted && !thinking.is_empty() && !text.is_empty() {
                            events.push(StreamEvent::Thinking(thinking.clone()));
                            thinking_emitted = true;
                            self.conversation[assistant_idx].thinking = Some(thinking.clone());
                        }
                        if !text.is_empty() {
                            events.push(StreamEvent::TextChunk(text.clone()));
                            self.conversation[assistant_idx].content.push_str(&text);
                        }
                    }

                    if let Some(calls) = delta.tool_calls {
                        for call_chunk in calls {
                            let index = call_chunk.index.unwrap_or(0);
                            let partial = tool_calls.entry(index).or_default();
                            if let Some(id) = call_chunk.id {
                                partial.id.push_str(&id);
                            }
                            if let Some(func) = call_chunk.function {
                                if let Some(name) = func.name {
                                    partial.name.push_str(&name);
                                }
                                if let Some(args) = func.arguments {
                                    partial.arguments.push_str(&args);
                                }
                            }
                        }
                    }
                }
            }
        }

        if !thinking_emitted && !thinking.is_empty() {
            events.push(StreamEvent::Thinking(thinking.clone()));
            self.conversation[assistant_idx].thinking = Some(thinking);
        }

        let mut completed: Vec<ToolCall> = tool_calls
            .into_iter()
            .map(|(_, p)| p.to_tool_call())
            .filter(|c| !c.function.name.is_empty())
            .collect();
        completed.sort_by(|a, b| a.id.cmp(&b.id));
        if !completed.is_empty() {
            self.conversation[assistant_idx].tool_calls = Some(
                completed
                    .iter()
                    .map(|call| ToolCallChunk {
                        index: None,
                        id: Some(call.id.clone()),
                        function: Some(ToolCallFunction {
                            name: Some(call.function.name.clone()),
                            arguments: Some(call.function.arguments.clone()),
                        }),
                    })
                    .collect(),
            );
        }

        Ok((events, completed))
    }

    fn last_assistant_text(&self) -> Result<String> {
        self.conversation
            .iter()
            .rev()
            .find(|m| m.role == Role::Assistant)
            .map(|m| m.content.clone())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow!("No assistant response"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use llm::Mock;

    #[test]
    fn test_base_agent_construction() {
        let api = API::Mock(Mock::new());
        let tool_context = ToolContext {
            cwd: std::env::current_dir().unwrap(),
            api: api.clone(),
            token: "test-token".to_string(),
            model: "test-model".to_string(),
            thinking: None,
        };
        let mut agent = BaseAgent::new(
            api,
            "test-token".to_string(),
            "test-model".to_string(),
            tool_context,
        );
        agent.set_max_iterations(5);
        agent.register_tool(crate::tools::files::ReadFileTool);
        assert_eq!(agent.max_iterations, 5);
        assert_eq!(agent.conversation().len(), 0);
    }
}
