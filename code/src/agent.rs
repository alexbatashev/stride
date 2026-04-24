use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use anyhow::{Result, anyhow};
use futures::StreamExt;
use llm::{API, CompletionRequest, Message, Role, StreamResponseChunk, UnnamedToolChoice};
use serde_json::Value;
use std::path::PathBuf;
use tokio::sync::oneshot;

use crate::{
    agent_capnp::event_sink,
    config::{Config, ProviderConfig, ProviderType, ThinkingConfig},
    tools::files::{BashTool, EditFileTool, ListFilesTool, ReadFileTool},
    tools::{ToolCall, ToolContext, ToolRegistry},
};

pub struct ConfirmChannel {
    pending: RefCell<Option<oneshot::Sender<bool>>>,
}

impl ConfirmChannel {
    pub fn new() -> Self {
        Self {
            pending: RefCell::new(None),
        }
    }

    pub fn set_pending(&self, sender: oneshot::Sender<bool>) {
        *self.pending.borrow_mut() = Some(sender);
    }

    pub fn resolve(&self, answer: bool) {
        if let Some(tx) = self.pending.borrow_mut().take() {
            let _ = tx.send(answer);
        }
    }
}

pub struct Agent {
    api: API,
    model: String,
    token: String,
    tool_registry: ToolRegistry,
    conversation: Vec<Message>,
    max_iterations: usize,
    thinking: Option<ThinkingConfig>,
    sink: event_sink::Client,
    confirm_channel: Rc<ConfirmChannel>,
    tool_context: ToolContext,
    checkpoint: Option<CheckpointFn>,
    tool_display_names: HashMap<usize, String>,
}

type CheckpointFn = Rc<dyn Fn(Vec<Message>, HashMap<usize, String>)>;

impl Agent {
    pub fn from_config(
        config: &Config,
        sink: event_sink::Client,
        confirm_channel: Rc<ConfirmChannel>,
        cwd: PathBuf,
        conversation: Option<Vec<Message>>,
        checkpoint: Option<CheckpointFn>,
    ) -> Result<Self> {
        let provider = config.get_default_provider()?;
        let api = create_api(provider)?;
        let token = provider
            .api_key
            .clone()
            .ok_or_else(|| anyhow!("API key not configured for provider '{}'", provider.name))?;

        let mut tool_registry = ToolRegistry::new();
        tool_registry.register(ReadFileTool);
        tool_registry.register(ListFilesTool);
        tool_registry.register(EditFileTool);
        tool_registry.register(BashTool);

        let system_message = Message {
            role: Role::System,
            content: build_system_prompt(&tool_registry),
            thinking: None,
            tool_call_id: None,
        };
        let conversation = conversation.unwrap_or_else(|| vec![system_message.clone()]);

        Ok(Self {
            api,
            model: config.default.model.clone(),
            token,
            tool_registry,
            conversation,
            max_iterations: config.agent.max_iterations,
            thinking: config.agent.thinking.clone(),
            sink,
            confirm_channel,
            tool_context: ToolContext { cwd },
            checkpoint,
            tool_display_names: HashMap::new(),
        })
    }

    pub async fn send_message(&mut self, text: String) -> Result<()> {
        if text.is_empty() {
            self.emit_done().await;
            return Ok(());
        }

        self.conversation.push(Message {
            role: Role::User,
            content: text,
            thinking: None,
            tool_call_id: None,
        });
        self.checkpoint();

        if let Err(e) = self.process_with_tools().await {
            self.emit_error(&format!("{}", e)).await;
            self.emit_done().await;
            return Err(e);
        }

        self.emit_done().await;
        Ok(())
    }

    pub async fn execute_command(&mut self, cmd: &str) -> Result<bool> {
        match cmd.split_whitespace().next().unwrap_or("") {
            "/quit" | "/q" => return Ok(true),
            other => {
                self.emit_error(&format!("Unknown command: {}", other))
                    .await;
            }
        }
        self.emit_done().await;
        Ok(false)
    }

    async fn process_with_tools(&mut self) -> Result<()> {
        for _ in 0..self.max_iterations {
            let request = self.build_completion_request();
            let mut stream = self.api.stream_completion(&self.token, request);
            let assistant_idx = self.begin_assistant_message();

            match self.collect_stream(&mut stream, assistant_idx).await {
                Ok(tool_calls) => {
                    if tool_calls.is_empty() {
                        return Ok(());
                    }

                    self.execute_tools(tool_calls).await?;
                }
                Err(e) => return Err(e),
            }
        }

        Err(anyhow!(
            "Reached maximum tool iteration limit ({})",
            self.max_iterations
        ))
    }

    async fn collect_stream(
        &mut self,
        stream: &mut std::pin::Pin<
            Box<dyn futures::Stream<Item = Result<StreamResponseChunk, llm::Error>> + Send>,
        >,
        assistant_idx: usize,
    ) -> Result<Vec<ToolCall>> {
        let mut thinking = String::new();
        let mut tool_calls: HashMap<usize, PartialToolCall> = HashMap::new();
        let mut thinking_sent = false;

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
                        if !thinking_sent && !thinking.is_empty() && !text.is_empty() {
                            self.emit_thinking(&thinking).await;
                            thinking_sent = true;
                            self.conversation[assistant_idx].thinking = Some(thinking.clone());
                        }
                        if !text.is_empty() {
                            self.emit_text_chunk(&text).await;
                            self.conversation[assistant_idx].content.push_str(&text);
                            self.checkpoint();
                        }
                    }

                    if let Some(calls) = delta.tool_calls {
                        for call_chunk in calls {
                            let index = call_chunk.index.unwrap_or(0);
                            let partial = tool_calls
                                .entry(index)
                                .or_insert_with(PartialToolCall::default);
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

        if !thinking_sent && !thinking.is_empty() {
            self.emit_thinking(&thinking).await;
            self.conversation[assistant_idx].thinking = Some(thinking);
            self.checkpoint();
        }

        let mut completed: Vec<ToolCall> = tool_calls
            .into_iter()
            .map(|(_, p)| p.to_tool_call())
            .filter(|c| !c.function.name.is_empty())
            .collect();
        completed.sort_by(|a, b| a.id.cmp(&b.id));

        Ok(completed)
    }

    async fn execute_tools(&mut self, tool_calls: Vec<ToolCall>) -> Result<()> {
        for call in tool_calls {
            let tool_name = call.function.name.clone();

            if call.call_type != "function" {
                self.push_tool_error(
                    &call.id,
                    &format!("Unsupported tool call type: {}", call.call_type),
                );
                continue;
            }

            self.emit_tool_call(&tool_name).await;

            let args = call
                .parsed_arguments()
                .unwrap_or(Value::Object(serde_json::Map::new()));

            if self.tool_registry.requires_confirmation(&tool_name) {
                let prompt = self
                    .tool_registry
                    .confirmation_prompt(&tool_name, &args)
                    .unwrap_or_else(|| format!("Execute {}", tool_name));

                let (tx, rx) = oneshot::channel();
                self.confirm_channel.set_pending(tx);
                self.emit_confirmation_required(&prompt).await;

                let approved = rx.await.unwrap_or(false);
                if !approved {
                    self.push_tool_error(&call.id, "User declined the operation");
                    continue;
                }
            }

            let result = if let Some(tool) = self.tool_registry.get(&tool_name) {
                tool.execute(args, &self.tool_context).await
            } else {
                crate::tools::ToolResult::error(format!("Unknown tool: {}", tool_name))
            };

            self.conversation.push(Message {
                role: Role::Tool,
                content: serde_json::to_string(&result)?,
                thinking: None,
                tool_call_id: Some(call.id),
            });
            self.tool_display_names
                .insert(self.conversation.len() - 1, tool_name);
            self.checkpoint();
        }
        Ok(())
    }

    fn push_tool_error(&mut self, call_id: &str, msg: &str) {
        self.conversation.push(Message {
            role: Role::Tool,
            content: serde_json::to_string(&crate::tools::ToolResult::error(msg))
                .unwrap_or_default(),
            thinking: None,
            tool_call_id: Some(call_id.to_string()),
        });
        self.tool_display_names
            .insert(self.conversation.len() - 1, "tool error".to_string());
        self.checkpoint();
    }

    async fn emit_text_chunk(&self, text: &str) {
        let mut req = self.sink.on_text_chunk_request();
        req.get().set_text(text);
        let _ = req.send().promise.await;
    }

    async fn emit_thinking(&self, text: &str) {
        let mut req = self.sink.on_thinking_request();
        req.get().set_text(text);
        let _ = req.send().promise.await;
    }

    async fn emit_tool_call(&self, name: &str) {
        let mut req = self.sink.on_tool_call_request();
        req.get().set_name(name);
        let _ = req.send().promise.await;
    }

    async fn emit_confirmation_required(&self, prompt: &str) {
        let mut req = self.sink.on_confirmation_required_request();
        req.get().set_prompt(prompt);
        let _ = req.send().promise.await;
    }

    async fn emit_error(&self, msg: &str) {
        let mut req = self.sink.on_error_request();
        req.get().set_message(msg);
        let _ = req.send().promise.await;
    }

    async fn emit_done(&self) {
        let _ = self.sink.on_done_request().send().promise.await;
    }

    fn build_completion_request(&self) -> CompletionRequest {
        let llm_tools: Vec<llm::Tool> = self
            .tool_registry
            .definitions()
            .into_iter()
            .map(|def| {
                let params = llm::FunctionParameters {
                    param_type: "object".to_string(),
                    properties: def
                        .function
                        .parameters
                        .properties
                        .into_iter()
                        .map(|(k, v)| {
                            (
                                k,
                                llm::FunctionProperty {
                                    r#type: v.property_type,
                                    description: v.description,
                                },
                            )
                        })
                        .collect(),
                    required: Some(def.function.parameters.required),
                };

                llm::Tool {
                    r#type: llm::ToolType::Function,
                    function: llm::Function {
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
            let thinking = llm::ThinkingConfig {
                thinking_type: thinking_config.thinking_type.clone(),
                budget_tokens: thinking_config.budget_tokens,
            };
            request = request.thinking(thinking);
        }

        request
    }

    pub fn reset_conversation(&mut self) {
        self.conversation = vec![Message {
            role: Role::System,
            content: build_system_prompt(&self.tool_registry),
            thinking: None,
            tool_call_id: None,
        }];
        self.checkpoint();
    }

    pub fn conversation(&self) -> &[Message] {
        &self.conversation
    }

    pub fn tool_display_names(&self) -> &HashMap<usize, String> {
        &self.tool_display_names
    }

    fn begin_assistant_message(&mut self) -> usize {
        self.conversation.push(Message {
            role: Role::Assistant,
            content: String::new(),
            thinking: None,
            tool_call_id: None,
        });
        self.checkpoint();
        self.conversation.len() - 1
    }

    fn checkpoint(&self) {
        if let Some(checkpoint) = &self.checkpoint {
            checkpoint(self.conversation.clone(), self.tool_display_names.clone());
        }
    }
}

fn create_api(provider: &ProviderConfig) -> Result<API> {
    match provider.provider_type {
        ProviderType::OpenAi => Ok(llm::OpenAI::new(&provider.base_url)),
        ProviderType::Anthropic => Ok(llm::Anthropic::new(&provider.base_url)),
        ProviderType::Ollama => Ok(llm::Ollama::new(&provider.base_url)),
    }
}

fn build_system_prompt(registry: &ToolRegistry) -> String {
    let mut prompt = r#"You are a helpful coding assistant. Your goal is to help users with software development tasks.

You have access to tools that can interact with the filesystem and execute commands. When you need to use a tool, the system will automatically invoke it based on your response.

Guidelines:
1. Use tools when you need to read files, explore directories, make edits, or run commands
2. Think step by step before taking action
3. Be concise but thorough in your responses
4. When editing files, make minimal, targeted changes
5. Always verify file contents before making changes

Available tools:
"#
    .to_string();

    for def in registry.definitions() {
        prompt.push_str(&format!(
            "\n- {}: {}\n",
            def.function.name, def.function.description
        ));

        if !def.function.parameters.properties.is_empty() {
            prompt.push_str("  Parameters:\n");
            for (name, prop) in &def.function.parameters.properties {
                let required = if def.function.parameters.required.contains(name) {
                    " (required)"
                } else {
                    " (optional)"
                };
                prompt.push_str(&format!(
                    "    - {}: {}{}\n",
                    name, prop.description, required
                ));
            }
        }
    }

    prompt.push_str(
        "\nWhen you need to use a tool, simply indicate it naturally. The system will handle the execution.",
    );

    prompt
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
            function: crate::tools::FunctionCall {
                name: self.name,
                arguments: self.arguments,
            },
        }
    }
}
