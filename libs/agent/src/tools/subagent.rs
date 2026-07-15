use std::sync::Arc;

use async_trait::async_trait;
use futures::StreamExt;
use llm::{Function, Tool as LlmTool};
use serde_json::{Value, json};

use crate::{
    AgentConfig, AutoDenyInteractionBroker, BaseAgent, EventKind, NoopEventSink, Tool, ToolContext,
    ToolDesc, ToolRegistry, TurnContext,
};

pub const SUBAGENT_NAME: &str = "subagent";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SubagentInteractionPolicy {
    AutoDeny,
    #[default]
    Bubble,
}

const SYSTEM_PROMPT: &str = "You are a focused subagent. Complete the assigned task using available tools when needed. Return a self-contained answer the main agent can act on. Do not ask the user questions directly.";

const BASE_DESCRIPTION: &str = "Spawn a subagent with a specific model to handle a focused subtask. Provide a short imperative `title` (3-6 words) shown in the UI, a detailed initial prompt, and choose a model appropriate for the task. The subagent runs independently and returns its final answer.";

pub struct SubAgentTool {
    desc: String,
    tool_registry: ToolRegistry,
    allowed_models: Vec<String>,
    system_prompt: String,
    requires_confirmation: bool,
    confirmation_prompt: Option<String>,
    interaction_policy: SubagentInteractionPolicy,
    /// Maximum nesting depth. A subagent whose parent path already has this
    /// length cannot spawn further subagents (see [`SubAgentTool::run_subagent`]).
    max_depth: usize,
}

#[derive(ToolDesc)]
struct SubAgentParams {
    /// Short human-readable task title, 3-6 words, imperative
    /// (e.g. "Research flight options"). Shown in the UI's subagent list.
    title: String,
    /// Detailed initial prompt for the subagent.
    prompt: String,
    /// Registry key of the model the subagent should use.
    model: String,
}

impl SubAgentTool {
    pub fn new(
        tool_registry: ToolRegistry,
        allowed_models: Vec<String>,
        guidelines: &str,
        max_depth: usize,
    ) -> Self {
        Self {
            desc: build_description(allowed_models.as_slice(), guidelines),
            tool_registry,
            allowed_models,
            system_prompt: SYSTEM_PROMPT.to_string(),
            requires_confirmation: false,
            confirmation_prompt: None,
            interaction_policy: SubagentInteractionPolicy::Bubble,
            max_depth: max_depth.max(1),
        }
    }

    pub fn requiring_confirmation(mut self, prompt: &str) -> Self {
        self.requires_confirmation = true;
        self.confirmation_prompt = Some(prompt.to_string());
        self
    }

    pub fn interaction_policy(mut self, policy: SubagentInteractionPolicy) -> Self {
        self.interaction_policy = policy;
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
            interaction_policy: SubagentInteractionPolicy::Bubble,
            // Fixed-model subagents (the local code agent's explorer) stay
            // non-recursive.
            max_depth: 1,
        }
    }

    /// Tool registry for a spawned child. When the child could still spawn its
    /// own subagents (`parent_depth + 1 < max_depth`) it gets a nested
    /// `SubAgentTool`; the dynamic depth check bounds the actual chain length.
    fn child_tool_registry(&self) -> ToolRegistry {
        let mut registry = self.tool_registry.clone();
        registry.register(SubAgentTool {
            desc: self.desc.clone(),
            tool_registry: self.tool_registry.clone(),
            allowed_models: self.allowed_models.clone(),
            system_prompt: self.system_prompt.clone(),
            requires_confirmation: self.requires_confirmation,
            confirmation_prompt: self.confirmation_prompt.clone(),
            interaction_policy: self.interaction_policy,
            max_depth: self.max_depth,
        });
        registry.allow_tool(SUBAGENT_NAME);
        registry
    }

    async fn run_subagent(
        &self,
        config: Arc<AgentConfig>,
        args: Value,
        context: Option<ToolContext>,
    ) -> Value {
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

        // Depth is the length of the *parent's* path. A parent already at the
        // limit cannot spawn a deeper child.
        let parent_depth = context
            .as_ref()
            .map(|context| context.turn.agent_path.len())
            .unwrap_or(0);
        if parent_depth >= self.max_depth {
            return json!({
                "success": false,
                "error": "maximum subagent depth reached"
            });
        }

        let agent_id = config.id_gen.new_uuid_v7();
        if let Some(context) = &context {
            context.emit(EventKind::AgentSpawned {
                agent_id,
                parent_tool_call_id: context.tool_call_id.clone(),
                name: resolve_title(&args.title, &args.prompt),
                model: model.to_owned(),
            });
        }
        // Give the child a nested subagent tool only when it could still spawn
        // within the depth limit; otherwise its registry is the plain base.
        let child_registry = if parent_depth + 1 < self.max_depth {
            self.child_tool_registry()
        } else {
            self.tool_registry.clone()
        };
        let agent = BaseAgent::new_with_tools(
            model.to_string(),
            config.clone(),
            self.system_prompt.clone(),
            vec![],
            child_registry,
        );
        let mut content = String::new();
        let error = if let Some(context) = &context {
            let child_turn = match self.interaction_policy {
                SubagentInteractionPolicy::Bubble => context.child_turn(agent_id),
                SubagentInteractionPolicy::AutoDeny => context
                    .child_turn(agent_id)
                    .with_broker(Arc::new(crate::AutoDenyInteractionBroker)),
            };
            let mut stream = agent
                .make_turn(args.prompt, Vec::new(), child_turn.clone())
                .await;
            let mut error = None;
            while let Some(event) = stream.next().await {
                match event.kind {
                    EventKind::TextDelta { delta, .. } => content.push_str(&delta),
                    EventKind::RunFailed { error: message } => {
                        error = Some(message);
                        break;
                    }
                    EventKind::RunFinished | EventKind::RunCancelled => break,
                    _ => {}
                }
            }
            child_turn.emit(
                config.id_gen.as_ref(),
                EventKind::AgentFinished {
                    agent_id,
                    result: error.clone().unwrap_or_else(|| content.clone()),
                },
            );
            error
        } else {
            let child_turn = TurnContext::new(
                config.id_gen.new_uuid_v7(),
                Arc::new(NoopEventSink),
                Arc::new(AutoDenyInteractionBroker),
            );
            let mut stream = agent.make_turn(args.prompt, Vec::new(), child_turn).await;
            let mut error = None;
            while let Some(event) = stream.next().await {
                match event.kind {
                    EventKind::TextDelta { delta, .. } => content.push_str(&delta),
                    EventKind::RunFailed { error: message } => {
                        error = Some(message);
                        break;
                    }
                    EventKind::RunFinished | EventKind::RunCancelled => break,
                    _ => {}
                }
            }
            error
        };

        match error {
            Some(error) => json!({ "success": false, "error": error }),
            None => json!({ "success": true, "content": content }),
        }
    }
}

/// A subagent's UI title: the model-provided `title` if non-empty, else the
/// first line of the prompt truncated to ~60 chars, else a generic fallback.
fn resolve_title(title: &str, prompt: &str) -> String {
    let title = title.trim();
    if !title.is_empty() {
        return title.to_string();
    }
    let first_line = prompt.lines().next().unwrap_or("").trim();
    let truncated: String = first_line.chars().take(60).collect();
    if truncated.is_empty() {
        "Subagent".to_string()
    } else {
        truncated
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
        self.run_subagent(config, args, None).await
    }

    async fn execute_with_context(
        &self,
        config: Arc<AgentConfig>,
        args: Value,
        context: Option<ToolContext>,
    ) -> Value {
        self.run_subagent(config, args, context).await
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
                json!({ "title": self.readable_name, "prompt": args.prompt, "model": self.model }),
            )
            .await
    }

    async fn execute_with_context(
        &self,
        config: Arc<AgentConfig>,
        args: Value,
        context: Option<ToolContext>,
    ) -> Value {
        let args = match SubAgentPromptOnlyParams::decode(args) {
            Ok(args) => args,
            Err(error) => return json!({ "success": false, "error": error }),
        };
        self.inner
            .execute_with_context(
                config,
                json!({ "title": self.readable_name, "prompt": args.prompt, "model": self.model }),
                context,
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
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    use async_trait::async_trait;
    use llm::{
        CompletionChoice, Delta, Function, FunctionParameters, StreamResponseChunk, ToolCallChunk,
        ToolCallFunction,
    };

    use super::*;
    use crate::{DEFAULT_MODEL, ModelRegEntry, ModelRegistry};

    #[derive(Default)]
    struct RecordingSink(Mutex<Vec<crate::ThreadEvent>>);

    impl crate::EventSink for RecordingSink {
        fn emit(&self, event: crate::ThreadEvent) {
            self.0.lock().unwrap().push(event);
        }
    }

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
            let tool =
                SubAgentTool::new(ToolRegistry::new(), vec![DEFAULT_MODEL.to_string()], "", 1);
            let result = tool
                .execute(
                    config(&mock),
                    json!({ "title": "Inspect target", "prompt": "inspect this", "model": DEFAULT_MODEL }),
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
            let tool = SubAgentTool::new(registry, vec![DEFAULT_MODEL.to_string()], "", 1);

            let result = tool
                .execute(
                    config(&mock),
                    json!({ "title": "Run inner tool", "prompt": "run inner tool", "model": DEFAULT_MODEL }),
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
            let tool =
                SubAgentTool::new(ToolRegistry::new(), vec![DEFAULT_MODEL.to_string()], "", 1);
            let result = tool
                .execute(
                    config(&mock),
                    json!({ "title": "Inspect target", "prompt": "inspect this", "model": "other" }),
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

    #[test]
    fn forwards_child_events_with_agent_path() {
        futures::executor::block_on(async {
            let mock = llm::Mock::new().with_stream_chunks(vec![vec![text_chunk("child answer")]]);
            let config = config(&mock);
            let sink = Arc::new(RecordingSink::default());
            let turn = crate::TurnContext::new(
                uuid::Uuid::from_u128(1),
                sink.clone(),
                Arc::new(crate::InMemoryInteractionBroker::default()),
            );
            let context =
                crate::ToolContext::new(turn, "parent_call".to_owned(), config.id_gen.clone());
            let tool =
                SubAgentTool::new(ToolRegistry::new(), vec![DEFAULT_MODEL.to_owned()], "", 1);

            let result = tool
                .execute_with_context(
                    config,
                    json!({ "title": "Inspect target", "prompt": "inspect this", "model": DEFAULT_MODEL }),
                    Some(context),
                )
                .await;

            assert_eq!(
                result,
                json!({ "success": true, "content": "child answer" })
            );
            let events = sink.0.lock().unwrap();
            let (agent_id, spawned_name) = events
                .iter()
                .find_map(|event| match &event.kind {
                    crate::EventKind::AgentSpawned { agent_id, name, .. } => {
                        Some((*agent_id, name.clone()))
                    }
                    _ => None,
                })
                .unwrap();
            assert_eq!(spawned_name, "Inspect target");
            assert!(events.iter().any(|event| {
                event.agent_path == [agent_id]
                    && matches!(&event.kind, crate::EventKind::TextDelta { delta, .. } if delta == "child answer")
            }));
            assert!(events.iter().any(|event| {
                event.agent_path == [agent_id]
                    && matches!(event.kind, crate::EventKind::AgentFinished { agent_id: id, .. } if id == agent_id)
            }));
        });
    }

    #[test]
    fn blocks_spawn_beyond_max_depth() {
        futures::executor::block_on(async {
            let mock = llm::Mock::new();
            let config = config(&mock);
            let sink = Arc::new(RecordingSink::default());
            // A child (agent_path length 1) at max_depth 1 cannot spawn further.
            let turn = crate::TurnContext::new(
                uuid::Uuid::from_u128(1),
                sink,
                Arc::new(crate::InMemoryInteractionBroker::default()),
            )
            .child(uuid::Uuid::from_u128(9));
            let context = crate::ToolContext::new(turn, "call".to_owned(), config.id_gen.clone());
            let tool =
                SubAgentTool::new(ToolRegistry::new(), vec![DEFAULT_MODEL.to_owned()], "", 1);

            let result = tool
                .execute_with_context(
                    config,
                    json!({ "title": "Deeper", "prompt": "go deeper", "model": DEFAULT_MODEL }),
                    Some(context),
                )
                .await;

            assert_eq!(
                result,
                json!({ "success": false, "error": "maximum subagent depth reached" })
            );
        });
    }

    #[test]
    fn nested_subagent_reaches_depth_two() {
        futures::executor::block_on(async {
            // Child spawns a grandchild, which streams its answer; the child then
            // summarizes. Three sequential model requests over one mock.
            let mock = llm::Mock::new().with_stream_chunks(vec![
                vec![subagent_call_chunk(
                    r#"{"title":"Compare prices","prompt":"compare","model":"default"}"#,
                )],
                vec![text_chunk("grandchild answer")],
                vec![text_chunk("child summary")],
            ]);
            let config = config(&mock);
            let sink = Arc::new(RecordingSink::default());
            let turn = crate::TurnContext::new(
                uuid::Uuid::from_u128(1),
                sink.clone(),
                Arc::new(crate::InMemoryInteractionBroker::default()),
            );
            let context =
                crate::ToolContext::new(turn, "root_call".to_owned(), config.id_gen.clone());
            let tool =
                SubAgentTool::new(ToolRegistry::new(), vec![DEFAULT_MODEL.to_owned()], "", 2);

            let result = tool
                .execute_with_context(
                    config,
                    json!({ "title": "Plan trip", "prompt": "plan", "model": DEFAULT_MODEL }),
                    Some(context),
                )
                .await;

            assert_eq!(
                result,
                json!({ "success": true, "content": "child summary" })
            );
            let events = sink.0.lock().unwrap();
            assert!(
                events.iter().any(|event| matches!(
                    &event.kind,
                    crate::EventKind::AgentSpawned { name, .. } if name == "Compare prices"
                )),
                "grandchild must be spawned with its title"
            );
            assert!(
                events.iter().any(|event| event.agent_path.len() == 2),
                "grandchild events must carry a depth-2 agent_path"
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
            usage_observer: Arc::new(stride_agent::NoopUsageObserver),
            ..Default::default()
        })
    }

    fn subagent_call_chunk(arguments: &str) -> StreamResponseChunk {
        named_tool_call_chunk("subagent", arguments)
    }

    fn tool_call_chunk(arguments: &str) -> StreamResponseChunk {
        named_tool_call_chunk("approval_tool", arguments)
    }

    fn named_tool_call_chunk(name: &str, arguments: &str) -> StreamResponseChunk {
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
                            name: Some(name.to_string()),
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
