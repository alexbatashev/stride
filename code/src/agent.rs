use crate::{
    cli,
    config::{Config, ProviderConfig, ProviderType, ThinkingConfig},
    tools::{ToolCall, ToolRegistry},
};
use anyhow::{Result, anyhow};
use futures::StreamExt;
use llm::{API, CompletionRequest, Message, Role, StreamResponseChunk, UnnamedToolChoice};
use serde_json::Value;
use std::collections::HashMap;

pub struct Agent {
    api: API,
    model: String,
    token: String,
    tool_registry: ToolRegistry,
    conversation: Vec<Message>,
    max_iterations: usize,
    confirm_destructive: bool,
    thinking: Option<ThinkingConfig>,
}

impl Agent {
    pub fn new(
        api: API,
        model: String,
        token: String,
        tool_registry: ToolRegistry,
        config: &Config,
    ) -> Self {
        let system_message = Message {
            role: Role::System,
            content: build_system_prompt(&tool_registry),
            thinking: None,
            tool_call_id: None,
        };

        Self {
            api,
            model,
            token,
            tool_registry,
            conversation: vec![system_message],
            max_iterations: config.agent.max_iterations,
            confirm_destructive: config.agent.confirm_destructive,
            thinking: config.agent.thinking.clone(),
        }
    }

    /// Create an Agent from a Config
    pub fn from_config(config: &Config, tool_registry: ToolRegistry) -> Result<Self> {
        let provider = config.get_default_provider()?;
        let api = create_api(provider)?;
        let token = provider
            .api_key
            .clone()
            .ok_or_else(|| anyhow!("API key not configured for provider '{}'", provider.name))?;

        Ok(Self::new(
            api,
            config.default.model.clone(),
            token,
            tool_registry,
            config,
        ))
    }

    /// Run the main agent loop
    pub async fn run(&mut self) -> Result<()> {
        cli::print_welcome();

        loop {
            let Some(input) = cli::prompt_user() else {
                break Ok(());
            };

            if input.is_empty() {
                continue;
            }

            if input.starts_with('/') {
                match self.handle_command(&input).await? {
                    CommandResult::Continue => continue,
                    CommandResult::Exit => break Ok(()),
                }
            } else {
                self.conversation.push(Message {
                    role: Role::User,
                    content: input,
                    thinking: None,
                    tool_call_id: None,
                });

                if let Err(e) = self.process_with_tools().await {
                    cli::print_error(&format!("{}", e));
                }
            }
        }
    }

    /// Handle slash commands
    async fn handle_command(&mut self, input: &str) -> Result<CommandResult> {
        let parts: Vec<&str> = input.split_whitespace().collect();
        let cmd = parts.first().unwrap_or(&"");

        match *cmd {
            "/quit" | "/q" => Ok(CommandResult::Exit),
            "/clear" | "/c" => {
                // Reset conversation but keep system message
                let system_msg = self.conversation.remove(0);
                self.conversation.clear();
                self.conversation.push(system_msg);
                cli::print_info("Conversation history cleared.");
                Ok(CommandResult::Continue)
            }
            "/help" | "/h" => {
                cli::print_help();
                Ok(CommandResult::Continue)
            }
            _ => {
                cli::print_warning(&format!("Unknown command: {}", cmd));
                Ok(CommandResult::Continue)
            }
        }
    }

    /// The inner loop: keep calling LLM until no more tool calls
    async fn process_with_tools(&mut self) -> Result<()> {
        for _ in 0..self.max_iterations {
            let request = self.build_completion_request();
            let mut stream = self.api.stream_completion(&self.token, request);

            let result = self.collect_stream(&mut stream).await;

            match result {
                Ok((content, thinking, tool_calls)) => {
                    // content and thinking were already streamed in collect_stream
                    if tool_calls.is_empty() {
                        self.conversation.push(Message {
                            role: Role::Assistant,
                            content,
                            thinking: None,
                            tool_call_id: None,
                        });
                        return Ok(());
                    }

                    self.conversation.push(Message {
                        role: Role::Assistant,
                        content,
                        thinking,
                        tool_call_id: None,
                    });

                    self.execute_tools(tool_calls).await?;
                }
                Err(e) => {
                    // Remove the user message we added before the API call failed
                    // so the user can try again or rephrase
                    if self.conversation.len() > 1 {
                        self.conversation.pop();
                    }
                    cli::print_error(&format!("{}", e));
                    return Err(e);
                }
            }
        }

        Err(anyhow!(
            "Reached maximum tool iteration limit ({}). Stopping to prevent infinite loop.",
            self.max_iterations
        ))
    }

    /// Collect streaming response, extracting content, thinking, and tool calls
    async fn collect_stream(
        &self,
        stream: &mut std::pin::Pin<
            Box<dyn futures::Stream<Item = Result<StreamResponseChunk, llm::Error>> + Send>,
        >,
    ) -> Result<(String, Option<String>, Vec<ToolCall>)> {
        let mut content = String::new();
        let mut thinking = String::new();
        let mut tool_calls: HashMap<usize, PartialToolCall> = HashMap::new();
        let mut printed_anything = false;
        let mut thinking_printed = false;

        while let Some(item) = stream.next().await {
            // Check if the item is an error
            let chunk = match item {
                Ok(c) => c,
                Err(e) => {
                    return Err(anyhow!("API error: {}", e));
                }
            };

            for choice in chunk.choices {
                if let Some(delta) = choice.delta {
                    if let Some(thinking_text) = delta.thinking {
                        thinking.push_str(&thinking_text);
                    }

                    if let Some(text) = delta.content {
                        if !thinking_printed && !thinking.is_empty() && !text.is_empty() {
                            cli::print_thinking(&thinking);
                            thinking_printed = true;
                        }
                        if !printed_anything && !text.is_empty() {
                            print!("\n🤖 ");
                            printed_anything = true;
                        }
                        cli::print_stream(&text);
                        content.push_str(&text);
                    }

                    if let Some(calls) = delta.tool_calls {
                        for call_chunk in calls {
                            let index = call_chunk.index.unwrap_or(0);
                            let partial =
                                tool_calls.entry(index).or_insert_with(PartialToolCall::default);

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

        if printed_anything {
            println!();
        }

        if !thinking_printed && !thinking.is_empty() {
            cli::print_thinking(&thinking);
        }

        // Convert partial tool calls to complete ones
        let mut completed_calls: Vec<ToolCall> = tool_calls
            .into_iter()
            .map(|(_, partial)| partial.to_tool_call())
            .filter(|call| !call.function.name.is_empty())
            .collect();

        completed_calls.sort_by(|a, b| a.id.cmp(&b.id));

        Ok((content, (!thinking.is_empty()).then_some(thinking), completed_calls))
    }

    /// Execute tools and add results to conversation
    async fn execute_tools(&mut self, tool_calls: Vec<ToolCall>) -> Result<()> {
        for call in tool_calls {
            let tool_name = &call.function.name;
            if call.call_type != "function" {
                self.conversation.push(Message {
                    role: Role::Tool,
                    content: serde_json::to_string(&crate::tools::ToolResult::error(format!(
                        "Unsupported tool call type: {}",
                        call.call_type
                    )))?,
                    thinking: None,
                    tool_call_id: Some(call.id),
                });
                continue;
            }

            cli::print_tool_call(tool_name);

            let args = call
                .parsed_arguments()
                .unwrap_or(Value::Object(serde_json::Map::new()));

            if self.confirm_destructive && self.tool_registry.requires_confirmation(tool_name) {
                if let Some(prompt) = self.tool_registry.confirmation_prompt(tool_name, &args) {
                    println!();
                    if !cli::confirm(&prompt) {
                        self.conversation.push(Message {
                            role: Role::Tool,
                            content: serde_json::to_string(&crate::tools::ToolResult::error(
                                "User declined the operation",
                            ))?,
                            thinking: None,
                            tool_call_id: Some(call.id.clone()),
                        });
                        continue;
                    }
                }
            }

            let result = if let Some(tool) = self.tool_registry.get(tool_name) {
                tool.execute(args).await
            } else {
                crate::tools::ToolResult::error(format!("Unknown tool: {}", tool_name))
            };

            self.conversation.push(Message {
                role: Role::Tool,
                content: serde_json::to_string(&result)?,
                thinking: None,
                tool_call_id: Some(call.id),
            });
        }

        Ok(())
    }

    /// Build the completion request from current conversation
    fn build_completion_request(&self) -> CompletionRequest {
        // Convert ToolDefinition to llm::Tool
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
}

enum CommandResult {
    Continue,
    Exit,
}

/// Build the system prompt with tool descriptions
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

/// Create an API client from provider config
fn create_api(provider: &ProviderConfig) -> Result<API> {
    match provider.provider_type {
        ProviderType::OpenAi => Ok(llm::OpenAI::new(&provider.base_url)),
        ProviderType::Anthropic => Ok(llm::Anthropic::new(&provider.base_url)),
        ProviderType::Ollama => Ok(llm::Ollama::new(&provider.base_url)),
    }
}

/// Helper struct to collect partial tool call data from streaming
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
