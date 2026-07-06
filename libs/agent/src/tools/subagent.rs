use std::sync::Arc;

use async_trait::async_trait;
use futures::StreamExt;
use llm::{Function, Tool as LlmTool};
use serde_json::{Value, json};

use crate::{AgentConfig, AgentResponseChunk, BaseAgent, Tool, ToolDesc, ToolRegistry};

pub const SUBAGENT_NAME: &str = "subagent";

const SYSTEM_PROMPT: &str = "You are a focused subagent. Complete the assigned task using available tools when needed. Return a self-contained answer the main agent can act on. Do not ask the user questions directly.";

const BASE_DESCRIPTION: &str = "Spawn a subagent with a specific model to handle a focused subtask. Provide a detailed initial prompt and choose a model appropriate for the task. The subagent runs independently and returns its final answer.";

pub struct SubAgentTool {
    desc: String,
    tool_registry: ToolRegistry,
    allowed_models: Vec<String>,
    system_prompt: String,
    requires_confirmation: bool,
    confirmation_prompt: Option<String>,
}

#[derive(ToolDesc)]
struct SubAgentParams {
    /// Detailed initial prompt for the subagent.
    prompt: String,
    /// Registry key of the model the subagent should use.
    model: String,
}

impl SubAgentTool {
    pub fn new(tool_registry: ToolRegistry, allowed_models: Vec<String>, guidelines: &str) -> Self {
        Self {
            desc: build_description(allowed_models.as_slice(), guidelines),
            tool_registry,
            allowed_models,
            system_prompt: SYSTEM_PROMPT.to_string(),
            requires_confirmation: false,
            confirmation_prompt: None,
        }
    }

    pub fn requiring_confirmation(mut self, prompt: &str) -> Self {
        self.requires_confirmation = true;
        self.confirmation_prompt = Some(prompt.to_string());
        self
    }

    fn with_model(model: &str, system_prompt: &str, tool_registry: ToolRegistry) -> Self {
        Self {
            desc: String::new(),
            tool_registry,
            allowed_models: vec![model.to_string()],
            system_prompt: system_prompt.to_string(),
            requires_confirmation: false,
            confirmation_prompt: None,
        }
    }
}

fn build_description(allowed_models: &[String], guidelines: &str) -> String {
    let mut desc = BASE_DESCRIPTION.to_string();
    if !allowed_models.is_empty() {
        desc.push_str("\n\nAllowed models: ");
        desc.push_str(&allowed_models.join(", "));
    }
    let trimmed = guidelines.trim();
    if !trimmed.is_empty() {
        desc.push_str("\n\nModel selection guidelines:\n");
        desc.push_str(trimmed);
    }
    desc
}

#[async_trait(?Send)]
impl Tool for SubAgentTool {
    fn name(&self) -> &str {
        SUBAGENT_NAME
    }

    fn readable_name(&self) -> &str {
        "Subagent"
    }

    fn definition(&self) -> LlmTool {
        LlmTool {
            r#type: llm::ToolType::Function,
            function: Function {
                description: self.desc.clone(),
                name: self.name().to_string(),
                parameters: Some(SubAgentParams::function_parameters()),
            },
        }
    }

    async fn execute(&self, config: Arc<AgentConfig>, args: Value) -> Value {
        let args = match SubAgentParams::decode(args) {
            Ok(args) => args,
            Err(error) => return json!({ "success": false, "error": error }),
        };

        let model = args.model.trim();
        if model.is_empty() {
            return json!({ "success": false, "error": "model is required" });
        }
        if !self.allowed_models.iter().any(|allowed| allowed == model) {
            return json!({
                "success": false,
                "error": format!("model '{model}' is not allowed for subagents")
            });
        }
        if config.model_registry.get(model).is_none() {
            return json!({
                "success": false,
                "error": format!("unknown model '{model}'")
            });
        }

        let agent = BaseAgent::new_with_tools(
            model.to_string(),
            config,
            self.system_prompt.clone(),
            vec![],
            self.tool_registry.clone(),
        );
        let mut stream = agent.make_turn(args.prompt, Vec::new()).await;
        let mut content = String::new();

        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(AgentResponseChunk::Chunk(chunk)) => {
                    for choice in chunk.choices {
                        if let Some(delta_content) = choice.delta.and_then(|d| d.content) {
                            content.push_str(&delta_content);
                        }
                    }
                }
                Ok(AgentResponseChunk::Approval { approved, .. }) => {
                    let _ = approved.send(false);
                }
                Ok(AgentResponseChunk::Quiz { answered, .. }) => {
                    let _ = answered.send(vec![]);
                }
                Ok(
                    AgentResponseChunk::ToolStarted { .. }
                    | AgentResponseChunk::ToolFinished { .. },
                ) => {}
                Err(error) => return json!({ "success": false, "error": error.to_string() }),
            }
        }

        json!({ "success": true, "content": content })
    }

    fn requires_confirmation(&self) -> bool {
        self.requires_confirmation
    }

    fn confirmation_prompt(&self, _args: &Value) -> String {
        self.confirmation_prompt
            .clone()
            .unwrap_or_else(|| format!("Execute {}", self.name()))
    }
}

/// Subagent with a preset model registry key. Used by the local code agent's
/// explorer tool where the model is fixed at registration time.
pub struct FixedModelSubAgentTool {
    name: String,
    readable_name: String,
    desc: String,
    model: String,
    inner: SubAgentTool,
}

impl FixedModelSubAgentTool {
    pub fn new(
        name: &str,
        readable_name: &str,
        desc: &str,
        model: &str,
        system_prompt: &str,
        tool_registry: ToolRegistry,
    ) -> Self {
        Self {
            name: name.to_string(),
            readable_name: readable_name.to_string(),
            desc: desc.to_string(),
            model: model.to_string(),
            inner: SubAgentTool::with_model(model, system_prompt, tool_registry),
        }
    }
}

#[async_trait(?Send)]
impl Tool for FixedModelSubAgentTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn readable_name(&self) -> &str {
        &self.readable_name
    }

    fn definition(&self) -> LlmTool {
        LlmTool {
            r#type: llm::ToolType::Function,
            function: Function {
                description: self.desc.clone(),
                name: self.name.clone(),
                parameters: Some(SubAgentPromptOnlyParams::function_parameters()),
            },
        }
    }

    async fn execute(&self, config: Arc<AgentConfig>, args: Value) -> Value {
        let args = match SubAgentPromptOnlyParams::decode(args) {
            Ok(args) => args,
            Err(error) => return json!({ "success": false, "error": error }),
        };
        self.inner
            .execute(
                config,
                json!({ "prompt": args.prompt, "model": self.model }),
            )
            .await
    }
}

#[derive(ToolDesc)]
struct SubAgentPromptOnlyParams {
    prompt: String,
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use async_trait::async_trait;
    use llm::{
        CompletionChoice, Delta, Function, FunctionParameters, StreamResponseChunk, ToolCallChunk,
        ToolCallFunction,
    };

    use super::*;
    use crate::{DEFAULT_MODEL, ModelRegEntry, ModelRegistry};

    #[derive(Clone)]
    struct ApprovalTool {
        calls: Arc<AtomicUsize>,
    }

    #[async_trait(?Send)]
    impl Tool for ApprovalTool {
        fn name(&self) -> &str {
            "approval_tool"
        }

        fn readable_name(&self) -> &str {
            "Approval tool"
        }

        fn definition(&self) -> LlmTool {
            LlmTool {
                r#type: llm::ToolType::Function,
                function: Function {
                    description: "Test approval tool".to_string(),
                    name: self.name().to_string(),
                    parameters: Some(FunctionParameters {
                        param_type: "object".to_string(),
                        ..Default::default()
                    }),
                },
            }
        }

        async fn execute(&self, _config: Arc<AgentConfig>, _args: Value) -> Value {
            self.calls.fetch_add(1, Ordering::SeqCst);
            json!({ "success": true })
        }

        fn requires_confirmation(&self) -> bool {
            true
        }
    }

    #[test]
    fn returns_final_subagent_content() {
        futures::executor::block_on(async {
            let mock = llm::Mock::new()
                .with_stream_chunks(vec![vec![text_chunk("final "), text_chunk("answer")]]);
            let tool = SubAgentTool::new(ToolRegistry::new(), vec![DEFAULT_MODEL.to_string()], "");
            let result = tool
                .execute(
                    config(&mock),
                    json!({ "prompt": "inspect this", "model": DEFAULT_MODEL }),
                )
                .await;

            assert_eq!(
                result,
                json!({ "success": true, "content": "final answer" })
            );
            let messages = &mock.stream_requests()[0].messages;
            assert_eq!(messages[0].role, llm::Role::System);
            assert_eq!(messages[0].content, SYSTEM_PROMPT);
            assert_eq!(messages[1].content, "inspect this");
        });
    }

    #[test]
    fn denies_inner_approval_requests() {
        futures::executor::block_on(async {
            let calls = Arc::new(AtomicUsize::new(0));
            let mock = llm::Mock::new().with_stream_chunks(vec![
                vec![tool_call_chunk(r#"{"value":1}"#)],
                vec![text_chunk("done")],
            ]);
            let mut registry = ToolRegistry::new();
            registry.register(ApprovalTool {
                calls: calls.clone(),
            });
            let tool = SubAgentTool::new(registry, vec![DEFAULT_MODEL.to_string()], "");

            let result = tool
                .execute(
                    config(&mock),
                    json!({ "prompt": "run inner tool", "model": DEFAULT_MODEL }),
                )
                .await;

            assert_eq!(result, json!({ "success": true, "content": "done" }));
            assert_eq!(calls.load(Ordering::SeqCst), 0);
        });
    }

    #[test]
    fn rejects_disallowed_model() {
        futures::executor::block_on(async {
            let mock = llm::Mock::new();
            let tool = SubAgentTool::new(ToolRegistry::new(), vec![DEFAULT_MODEL.to_string()], "");
            let result = tool
                .execute(
                    config(&mock),
                    json!({ "prompt": "inspect this", "model": "other" }),
                )
                .await;
            assert_eq!(
                result,
                json!({
                    "success": false,
                    "error": "model 'other' is not allowed for subagents"
                })
            );
        });
    }

    fn config(mock: &llm::Mock) -> Arc<AgentConfig> {
        let mut registry = ModelRegistry::new();
        registry.add_model(
            DEFAULT_MODEL,
            ModelRegEntry {
                api: mock.clone().into(),
                token: String::new(),
                model_name: "mock-model".to_string(),
                reasoning_effort: None,
                vision: false,
            },
        );
        Arc::new(AgentConfig {
            model_registry: registry,
            max_iterations: 50,
            observer: Arc::new(stride_agent::NoopAgentObserver),
        })
    }

    fn tool_call_chunk(arguments: &str) -> StreamResponseChunk {
        StreamResponseChunk {
            id: "chunk".to_string(),
            object: "mock.stream".to_string(),
            created: 0,
            model: "mock-model".to_string(),
            system_fingerprint: None,
            usage: None,
            choices: vec![CompletionChoice {
                message: None,
                text: None,
                index: 0,
                delta: Some(Delta {
                    content: None,
                    thinking: None,
                    tool_calls: Some(vec![ToolCallChunk {
                        index: Some(0),
                        id: Some("call_1".to_string()),
                        call_type: None,
                        function: Some(ToolCallFunction {
                            name: Some("approval_tool".to_string()),
                            arguments: Some(arguments.to_string()),
                        }),
                    }]),
                }),
                logprobs: None,
                tool_calls: None,
                finish_reason: Some("tool_calls".to_string()),
            }],
        }
    }

    fn text_chunk(content: &str) -> StreamResponseChunk {
        StreamResponseChunk {
            id: "chunk".to_string(),
            object: "mock.stream".to_string(),
            created: 0,
            model: "mock-model".to_string(),
            system_fingerprint: None,
            usage: None,
            choices: vec![CompletionChoice {
                message: None,
                text: Some(content.to_string()),
                index: 0,
                delta: Some(Delta {
                    content: Some(content.to_string()),
                    thinking: None,
                    tool_calls: None,
                }),
                logprobs: None,
                tool_calls: None,
                finish_reason: Some("stop".to_string()),
            }],
        }
    }
}
