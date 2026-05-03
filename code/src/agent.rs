use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use anyhow::{Result, anyhow};
use friday_agent::{
    AgentConfig as FrameworkAgentConfig, AgentResponseChunk, BaseAgent, DEFAULT_MODEL,
    ModelRegEntry, ModelRegistry,
    tools::{
        explorer::{EXPLORER_MODEL, make_explorer},
        file::ReadFileTool,
        glob::GlobTool,
        patch::PatchTool,
        shell::ShellTool,
    },
};
use futures::{StreamExt, channel::oneshot};
use llm::{API, Message, Role};

use crate::{
    agent_capnp::event_sink,
    config::{Config, ProviderConfig, ProviderType},
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
    model: String,
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
        _cwd: PathBuf,
        conversation: Option<Vec<Message>>,
        saved_model: Option<String>,
        checkpoint: Option<CheckpointFn>,
    ) -> Result<Self> {
        let selection = initial_model_selection(config, saved_model).await?;
        let agent_config = build_framework_config(config, &selection)?;
        let base = BaseAgent::new(
            DEFAULT_MODEL.to_string(),
            agent_config,
            build_system_prompt(&registered_tool_definitions()),
            strip_system_prompt(conversation.unwrap_or_default()),
        );
        register_tools(&base);

        Ok(Self {
            config: config.clone(),
            base,
            provider: selection.provider,
            model: selection.model,
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

        let mut stream = self.base.make_turn(text).await;
        self.checkpoint();

        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(AgentResponseChunk::Chunk(chunk)) => {
                    self.emit_chunk(chunk).await;
                    self.checkpoint();
                }
                Ok(AgentResponseChunk::Approval { message, approved }) => {
                    self.confirm_channel.set_pending(approved);
                    self.emit_confirmation_required(&message).await;
                    self.checkpoint();
                }
                Err(e) => {
                    self.emit_error(&e.to_string()).await;
                    self.emit_done().await;
                    return Err(anyhow!(e.to_string()));
                }
            }
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

        let agent_config = build_framework_config(&self.config, &selection)?;
        self.base.set_config(agent_config);
        self.provider = provider.name;
        self.model = selection.model;

        self.emit_text_chunk(&format!("Model set to {}", self.model_key()))
            .await;
        self.checkpoint();
        Ok(())
    }

    async fn emit_chunk(&self, chunk: llm::StreamResponseChunk) {
        for choice in chunk.choices {
            if let Some(delta) = choice.delta {
                if let Some(thinking) = delta.thinking {
                    if !thinking.is_empty() {
                        self.emit_thinking(&thinking).await;
                    }
                }

                if let Some(content) = delta.content {
                    if !content.is_empty() {
                        self.emit_text_chunk(&content).await;
                    }
                }

                if let Some(tool_calls) = delta.tool_calls {
                    for call in tool_calls {
                        if let Some(function) = call.function {
                            if let Some(name) = function.name {
                                if !name.is_empty() {
                                    self.emit_tool_call(&name).await;
                                }
                            }
                        }
                    }
                }
            }
        }
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
        self.base
            .set_thread(build_system_prompt(&registered_tool_definitions()), vec![]);
        self.checkpoint();
    }

    pub fn reset_model(&mut self, config: &Config) -> Result<()> {
        let selection = ModelSelection {
            provider: config.default.provider.clone(),
            model: config.default.model.clone(),
        };
        let provider = config.get_default_provider()?;
        self.base
            .set_config(build_framework_config(config, &selection)?);
        self.provider = provider.name.clone();
        self.model = selection.model;
        Ok(())
    }

    pub fn conversation(&self) -> Vec<Message> {
        self.base.thread()
    }

    pub fn tool_display_names(&self) -> HashMap<usize, String> {
        self.base.tool_display_names()
    }

    pub fn model_key(&self) -> String {
        format!("{}/{}", self.provider, self.model)
    }

    fn checkpoint(&self) {
        if let Some(checkpoint) = &self.checkpoint {
            checkpoint(
                self.base.thread(),
                self.base.tool_display_names(),
                self.model_key(),
            );
        }
    }
}

fn register_tools(base: &BaseAgent) {
    base.register_tool(ReadFileTool);
    base.register_tool(GlobTool);
    base.register_tool(PatchTool);
    base.register_tool(ShellTool);
    base.register_tool(make_explorer());
}

fn registered_tool_definitions() -> Vec<llm::Tool> {
    let registry = friday_agent::ToolRegistry::new();
    let base = BaseAgent::new_with_tools(
        DEFAULT_MODEL.to_string(),
        Arc::new(FrameworkAgentConfig {
            model_registry: ModelRegistry::new(),
            max_iterations: 1,
        }),
        String::new(),
        vec![],
        registry,
    );
    register_tools(&base);
    base.tool_definitions()
}

fn build_framework_config(
    config: &Config,
    selection: &ModelSelection,
) -> Result<Arc<FrameworkAgentConfig>> {
    let mut registry = ModelRegistry::new();
    let provider = config.get_provider(&selection.provider)?;
    registry.add_model(
        DEFAULT_MODEL,
        model_entry(provider, &selection.model, config)?,
    );

    let explorer_selection = config
        .agent
        .subagent_model
        .as_deref()
        .map(parse_model_selection)
        .transpose()?
        .unwrap_or_else(|| selection.clone());
    let explorer_provider = config.get_provider(&explorer_selection.provider)?;
    registry.add_model(
        EXPLORER_MODEL,
        model_entry(explorer_provider, &explorer_selection.model, config)?,
    );

    Ok(Arc::new(FrameworkAgentConfig {
        model_registry: registry,
        max_iterations: config.agent.max_iterations,
    }))
}

fn model_entry(provider: &ProviderConfig, model: &str, config: &Config) -> Result<ModelRegEntry> {
    Ok(ModelRegEntry {
        api: create_api(provider)?,
        token: provider_token(provider)?,
        model_name: model.to_string(),
        thinking: config.agent.thinking,
    })
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

fn strip_system_prompt(mut conversation: Vec<Message>) -> Vec<Message> {
    if conversation
        .first()
        .is_some_and(|message| message.role == Role::System)
    {
        conversation.remove(0);
    }
    conversation
}

fn build_system_prompt(tools: &[llm::Tool]) -> String {
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

    for def in tools {
        prompt.push_str(&format!(
            "\n- {}: {}\n",
            def.function.name, def.function.description
        ));

        if let Some(parameters) = &def.function.parameters {
            if !parameters.properties.is_empty() {
                prompt.push_str("  Parameters:\n");
                for (name, prop) in &parameters.properties {
                    let required = if parameters
                        .required
                        .as_ref()
                        .is_some_and(|required| required.contains(name))
                    {
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
    }

    prompt.push_str(
        "\nWhen you need to use a tool, simply indicate it naturally. The system will handle the execution.",
    );

    prompt
}
