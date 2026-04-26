use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use anyhow::{Result, anyhow};
use llm::{API, Message, Role, ThinkingConfig};
use serde_json::Value;
use tokio::sync::oneshot;

use crate::{
    agent_capnp::event_sink,
    config::{Config, ProviderConfig, ProviderType},
    tools::base_agent::{BaseAgent, StreamEvent},
    tools::explorer::ContextExplorerTool,
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
    config: Config,
    base: BaseAgent,
    provider: String,
    sink: event_sink::Client,
    confirm_channel: Rc<ConfirmChannel>,
    checkpoint: Option<CheckpointFn>,
}

type CheckpointFn = Rc<dyn Fn(Vec<Message>, HashMap<usize, String>, String)>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelSelection {
    pub provider: String,
    pub model: String,
}

impl Agent {
    pub async fn from_config(
        config: &Config,
        sink: event_sink::Client,
        confirm_channel: Rc<ConfirmChannel>,
        cwd: PathBuf,
        conversation: Option<Vec<Message>>,
        saved_model: Option<String>,
        checkpoint: Option<CheckpointFn>,
    ) -> Result<Self> {
        let selection = initial_model_selection(config, saved_model).await?;
        let provider = config.get_provider(&selection.provider)?;
        let api = create_api(provider)?;
        let token = provider_token(provider)?;

        let thinking = config.agent.thinking.as_ref().map(|tc| ThinkingConfig {
            thinking_type: tc.thinking_type.clone(),
            budget_tokens: tc.budget_tokens,
        });

        let tool_context = if let Some(ref subagent_model_str) = config.agent.subagent_model {
            let sub_selection = parse_model_selection(subagent_model_str)?;
            let sub_provider = config.get_provider(&sub_selection.provider)?;
            let sub_api = create_api(sub_provider)?;
            let sub_token = provider_token(sub_provider)?;
            ToolContext {
                cwd: cwd.clone(),
                api: sub_api,
                token: sub_token,
                model: sub_selection.model,
                thinking: thinking.clone(),
            }
        } else {
            ToolContext {
                cwd: cwd.clone(),
                api: api.clone(),
                token: token.clone(),
                model: selection.model.clone(),
                thinking: thinking.clone(),
            }
        };

        let mut base = BaseAgent::new(api, token, selection.model, tool_context);
        base.set_max_iterations(config.agent.max_iterations);
        base.set_thinking(thinking);

        base.register_tool(ReadFileTool);
        base.register_tool(ListFilesTool);
        base.register_tool(EditFileTool);
        base.register_tool(BashTool);
        base.register_tool(ContextExplorerTool);

        if let Some(saved_conversation) = conversation {
            base.set_conversation(saved_conversation);
        } else {
            base.push_message(Message {
                role: Role::System,
                content: build_system_prompt(base.tool_registry()),
                thinking: None,
                tool_calls: None,
                tool_call_id: None,
            });
        }

        Ok(Self {
            config: config.clone(),
            base,
            provider: provider.name.clone(),
            sink,
            confirm_channel,
            checkpoint,
        })
    }

    pub async fn send_message(&mut self, text: String) -> Result<()> {
        if text.is_empty() {
            self.emit_done().await;
            return Ok(());
        }

        self.base.push_message(Message {
            role: Role::User,
            content: text,
            thinking: None,
            tool_calls: None,
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
            "/model" => {
                self.select_model(cmd).await?;
            }
            other => {
                self.emit_error(&format!("Unknown command: {}", other))
                    .await;
            }
        }
        self.emit_done().await;
        Ok(false)
    }

    async fn select_model(&mut self, cmd: &str) -> Result<()> {
        let selected = cmd
            .split_whitespace()
            .nth(1)
            .ok_or_else(|| anyhow!("Usage: /model <provider>/<model>"))?;
        let selection = parse_model_selection(selected)?;
        let provider = self.config.get_provider(&selection.provider)?.clone();
        let api = create_api(&provider)?;
        let token = provider_token(&provider)?;
        ensure_model_available(
            &api,
            &token,
            &selection.model,
            AvailabilityMode::AllowUnknown,
        )
        .await?;

        self.base.api = api;
        self.base.token = token;
        self.base.model = selection.model;
        self.provider = provider.name.clone();

        if self.config.agent.subagent_model.is_none() {
            self.base.tool_context.api = self.base.api.clone();
            self.base.tool_context.token = self.base.token.clone();
            self.base.tool_context.model = self.base.model.clone();
        }

        self.emit_text_chunk(&format!("Model set to {}", self.model_key()))
            .await;
        Ok(())
    }

    async fn process_with_tools(&mut self) -> Result<()> {
        for _ in 0..self.base.max_iterations {
            let (events, tool_calls) = self.base.collect_response().await?;

            for event in events {
                match event {
                    StreamEvent::Thinking(text) => self.emit_thinking(&text).await,
                    StreamEvent::TextChunk(text) => self.emit_text_chunk(&text).await,
                }
            }
            self.checkpoint();

            if tool_calls.is_empty() {
                return Ok(());
            }

            self.execute_tools_with_confirmation(tool_calls).await?;
        }

        Err(anyhow!(
            "Reached maximum tool iteration limit ({})",
            self.base.max_iterations
        ))
    }

    async fn execute_tools_with_confirmation(&mut self, tool_calls: Vec<ToolCall>) -> Result<()> {
        for call in tool_calls {
            let tool_name = call.function.name.clone();

            if call.call_type != "function" {
                self.base.push_tool_error(
                    &call.id,
                    &format!("Unsupported tool call type: {}", call.call_type),
                );
                self.checkpoint();
                continue;
            }

            self.emit_tool_call(&tool_name).await;

            let args = call
                .parsed_arguments()
                .unwrap_or(Value::Object(serde_json::Map::new()));

            if self.base.tool_registry.requires_confirmation(&tool_name) {
                let prompt = self
                    .base
                    .tool_registry
                    .confirmation_prompt(&tool_name, &args)
                    .unwrap_or_else(|| format!("Execute {}", tool_name));

                let (tx, rx) = oneshot::channel();
                self.confirm_channel.set_pending(tx);
                self.emit_confirmation_required(&prompt).await;

                let approved = rx.await.unwrap_or(false);
                if !approved {
                    self.base
                        .push_tool_error(&call.id, "User declined the operation");
                    self.checkpoint();
                    continue;
                }
            }

            self.base.execute_tool(call).await?;
            self.checkpoint();
        }
        Ok(())
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

    pub fn reset_conversation(&mut self) {
        let system_message = Message {
            role: Role::System,
            content: build_system_prompt(self.base.tool_registry()),
            thinking: None,
            tool_calls: None,
            tool_call_id: None,
        };
        self.base.set_conversation(vec![system_message]);
        self.checkpoint();
    }

    pub fn reset_model(&mut self, config: &Config) -> Result<()> {
        let provider = config.get_default_provider()?;
        let api = create_api(provider)?;
        let token = provider_token(provider)?;
        self.base.api = api;
        self.base.token = token;
        self.base.model = config.default.model.clone();
        self.provider = provider.name.clone();

        if config.agent.subagent_model.is_none() {
            self.base.tool_context.api = self.base.api.clone();
            self.base.tool_context.token = self.base.token.clone();
            self.base.tool_context.model = self.base.model.clone();
        }

        Ok(())
    }

    pub fn conversation(&self) -> &[Message] {
        self.base.conversation()
    }

    pub fn tool_display_names(&self) -> &HashMap<usize, String> {
        self.base.tool_display_names()
    }

    pub fn model_key(&self) -> String {
        format!("{}/{}", self.provider, self.base.model)
    }

    fn checkpoint(&self) {
        if let Some(checkpoint) = &self.checkpoint {
            checkpoint(
                self.base.conversation().to_vec(),
                self.base.tool_display_names().clone(),
                self.model_key(),
            );
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

fn provider_token(provider: &ProviderConfig) -> Result<String> {
    provider
        .api_key
        .clone()
        .ok_or_else(|| anyhow!("API key not configured for provider '{}'", provider.name))
}

async fn initial_model_selection(
    config: &Config,
    saved_model: Option<String>,
) -> Result<ModelSelection> {
    let default = ModelSelection {
        provider: config.default.provider.clone(),
        model: config.default.model.clone(),
    };
    let Some(saved_model) = saved_model else {
        return Ok(default);
    };
    let Ok(selection) = parse_model_selection(&saved_model) else {
        return Ok(default);
    };
    let Ok(provider) = config.get_provider(&selection.provider) else {
        return Ok(default);
    };
    let Ok(api) = create_api(provider) else {
        return Ok(default);
    };
    let Ok(token) = provider_token(provider) else {
        return Ok(default);
    };
    if ensure_model_available(
        &api,
        &token,
        &selection.model,
        AvailabilityMode::FallbackOnUnknown,
    )
    .await
    .is_ok()
    {
        Ok(selection)
    } else {
        Ok(default)
    }
}

#[derive(Copy, Clone)]
enum AvailabilityMode {
    AllowUnknown,
    FallbackOnUnknown,
}

async fn ensure_model_available(
    api: &API,
    token: &str,
    model: &str,
    mode: AvailabilityMode,
) -> Result<()> {
    let models = match api.list_models(token).await {
        Ok(models) => models,
        Err(e) => {
            return match mode {
                AvailabilityMode::AllowUnknown => Ok(()),
                AvailabilityMode::FallbackOnUnknown => Err(anyhow!("failed to list models: {}", e)),
            };
        }
    };
    if models.iter().any(|item| item.id == model) {
        Ok(())
    } else {
        Err(anyhow!("model '{}' is not available", model))
    }
}

fn parse_model_selection(value: &str) -> Result<ModelSelection> {
    let (provider, model) = value
        .split_once('/')
        .ok_or_else(|| anyhow!("model must be qualified as <provider>/<model>"))?;
    if provider.is_empty() || model.is_empty() {
        return Err(anyhow!("model must be qualified as <provider>/<model>"));
    }
    Ok(ModelSelection {
        provider: provider.to_string(),
        model: model.to_string(),
    })
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
