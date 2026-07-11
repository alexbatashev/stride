use std::collections::{BTreeMap, HashMap, VecDeque};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::{cell::RefCell, rc::Rc};

use async_stream::stream;
use futures::channel::{mpsc, oneshot};
use futures::future::Either;
use futures::stream::FuturesUnordered;
use futures::{Stream, StreamExt};
use llm::{
    API, CompletionRequest, ImageSource, Message, OpenAI, ReasoningEffort, StreamResponseChunk,
    ToolCallChunk, ToolCallFunction,
};
use serde_json::Value;
use serde_json::json;
use thiserror::Error;

use crate::determinism::{Clock, IdGen, SystemClock, SystemIdGen};
use crate::events::{EventKind, EventSink, MessageRole, ThreadEvent, ToolContext, TurnContext};
use crate::tools::search::{SearchEntry, SearchTool};
use crate::{QuizQuestion, Tool, ToolRegistry};

pub const DEFAULT_MODEL: &str = "default";

/// Registry key reserved for the text embedding model. When a model is
/// registered under this name (pointing at an OpenAI- or Ollama-compatible
/// provider), features like the memory palace use it to embed text.
pub const EMBEDDING_MODEL: &str = "embeddings";

/// Registry key reserved for the audio transcription model. When a model is
/// registered under this name (pointing at an OpenAI-compatible provider with a
/// Whisper-style endpoint), voice input from the web UI and Telegram is
/// transcribed before being handed to the agent.
pub const TRANSCRIPTION_MODEL: &str = "transcription";

pub struct BaseAgent(Rc<RefCell<BaseAgentInner>>);

#[derive(Clone, Debug, Default)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub model: String,
    pub provider: String,
}

pub trait UsageObserver: Send + Sync {
    fn token_usage(&self, _usage: TokenUsage) {}
}

#[derive(Default)]
pub struct NoopUsageObserver;

impl UsageObserver for NoopUsageObserver {}

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("Missing model designated as '{0}'")]
    MissingProvider(String),
    #[error("Network error: {0}")]
    NetworkError(String),
}

impl From<llm::Error> for AgentError {
    fn from(err: llm::Error) -> Self {
        AgentError::NetworkError(err.to_string())
    }
}

enum AgentResponseChunk {
    Chunk(StreamResponseChunk),
    ToolStarted {
        tool_call_id: String,
        name: String,
        arguments: String,
    },
    ToolFinished {
        tool_call_id: String,
        name: String,
        result: String,
    },
    Approval {
        message: String,
        approved: oneshot::Sender<bool>,
    },
    Quiz {
        questions: Vec<QuizQuestion>,
        answered: oneshot::Sender<Vec<String>>,
    },
}

type AgentResponseStream =
    Pin<Box<dyn Stream<Item = Result<AgentResponseChunk, AgentError>> + 'static>>;
pub type ThreadEventStream = Pin<Box<dyn Stream<Item = ThreadEvent> + 'static>>;

struct TeeEventSink {
    sink: Arc<dyn EventSink>,
    sender: mpsc::UnboundedSender<ThreadEvent>,
}

impl EventSink for TeeEventSink {
    fn emit(&self, event: ThreadEvent) {
        self.sink.emit(event.clone());
        let _ = self.sender.unbounded_send(event);
    }
}

pub struct AgentConfig {
    pub model_registry: ModelRegistry,
    pub max_iterations: usize,
    pub usage_observer: Arc<dyn UsageObserver>,
    pub clock: Arc<dyn Clock>,
    pub id_gen: Arc<dyn IdGen>,
    pub max_concurrent_tools: usize,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            model_registry: ModelRegistry::default(),
            max_iterations: 0,
            usage_observer: Arc::new(NoopUsageObserver),
            clock: Arc::new(SystemClock),
            id_gen: Arc::new(SystemIdGen),
            max_concurrent_tools: 4,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct ModelRegistry {
    models: HashMap<String, ModelRegEntry>,
    providers: HashMap<String, String>,
}

#[derive(Clone, Debug)]
pub struct ModelRegEntry {
    pub api: API,
    pub token: String,
    pub model_name: String,
    /// Reasoning effort to request from the model. `None` disables reasoning.
    pub reasoning_effort: Option<ReasoningEffort>,
    /// Whether the model accepts image inputs. Gates image attachment and the
    /// `attach_image` tool on the server side.
    pub vision: bool,
}

impl ModelRegEntry {
    pub fn openai_compatible(
        endpoint: impl AsRef<str>,
        token: impl Into<String>,
        model_name: impl Into<String>,
    ) -> Self {
        Self {
            api: API::OpenAI(OpenAI::new(endpoint.as_ref())),
            token: token.into(),
            model_name: model_name.into(),
            reasoning_effort: None,
            vision: true,
        }
    }
}

fn provider_label(entry: &ModelRegEntry) -> &'static str {
    match &entry.api {
        API::OpenAI(_) => "openai",
        API::Anthropic(_) => "anthropic",
        API::Ollama(_) => "ollama",
        API::Mock(_) => "mock",
    }
}

struct BaseAgentInner {
    tool_registry: ToolRegistry,
    thread: Vec<Message>,
    tool_display_names: HashMap<usize, String>,
    config: Arc<AgentConfig>,
    model: String,
    search_slot: Arc<Mutex<Vec<llm::Tool>>>,
    searchable_entries: Arc<Mutex<Vec<SearchEntry>>>,
    searchable_tools_preview_limit: Arc<Mutex<usize>>,
}

#[derive(Default)]
struct PartialToolCall {
    id: String,
    name: String,
    arguments: String,
}

impl ModelRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_model(&mut self, name: &str, entry: ModelRegEntry) {
        let provider = provider_label(&entry);
        self.add_model_with_provider(name, entry, provider);
    }

    pub fn add_model_with_provider(
        &mut self,
        name: &str,
        entry: ModelRegEntry,
        provider: impl Into<String>,
    ) {
        self.models.insert(name.to_string(), entry);
        self.providers.insert(name.to_string(), provider.into());
    }

    pub fn get_or_default(&self, name: &str) -> &ModelRegEntry {
        if let Some(entry) = self.models.get(name) {
            entry
        } else if let Some(entry) = self.models.get(DEFAULT_MODEL) {
            entry
        } else {
            panic!("ModelRegistry must always have a 'default' model");
        }
    }

    pub fn get(&self, name: &str) -> Option<&ModelRegEntry> {
        self.models.get(name)
    }

    pub fn provider(&self, name: &str) -> Option<&str> {
        self.providers.get(name).map(String::as_str)
    }

    pub fn entries(&self) -> impl Iterator<Item = (&str, &ModelRegEntry)> {
        self.models
            .iter()
            .map(|(name, entry)| (name.as_str(), entry))
    }

    /// The model designated for text embeddings, if one is registered under the
    /// [`EMBEDDING_MODEL`] key.
    pub fn embedding(&self) -> Option<&ModelRegEntry> {
        self.models.get(EMBEDDING_MODEL)
    }

    /// The model designated for audio transcription, if one is registered under
    /// the [`TRANSCRIPTION_MODEL`] key.
    pub fn transcription(&self) -> Option<&ModelRegEntry> {
        self.models.get(TRANSCRIPTION_MODEL)
    }
}

impl BaseAgent {
    pub fn new(
        model: String,
        config: Arc<AgentConfig>,
        system_prompt: String,
        thread: Vec<Message>,
    ) -> Self {
        Self::new_with_tools(model, config, system_prompt, thread, ToolRegistry::new())
    }

    pub fn new_with_tools(
        model: String,
        config: Arc<AgentConfig>,
        system_prompt: String,
        thread: Vec<Message>,
        tool_registry: ToolRegistry,
    ) -> Self {
        let thread = thread_with_system_prompt(system_prompt, thread);

        Self(Rc::new(RefCell::new(BaseAgentInner {
            tool_registry,
            thread,
            tool_display_names: HashMap::new(),
            config,
            model,
            search_slot: Arc::new(Mutex::new(Vec::new())),
            searchable_entries: Arc::new(Mutex::new(Vec::new())),
            searchable_tools_preview_limit: Arc::new(Mutex::new(20)),
        })))
    }

    pub fn register_tool(&self, tool: impl Tool + 'static) {
        self.0.borrow_mut().tool_registry.register(tool);
    }

    /// Register a tool that is hidden from the LLM by default. The LLM can
    /// discover it via `search_tools` and use it for one turn.
    pub fn register_searchable_tool(&self, tool: impl Tool + 'static) {
        let mut lock = self.0.borrow_mut();
        let entry = SearchEntry {
            name: tool.name().to_string(),
            description: tool.definition().function.description.clone(),
            definition: tool.definition(),
            category: tool.searchable_category(),
        };
        lock.searchable_entries.lock().unwrap().push(entry);
        lock.tool_registry.register_searchable(tool);

        if lock.tool_registry.get("search_tools").is_none() {
            let search_tool = SearchTool {
                entries: lock.searchable_entries.clone(),
                slot: lock.search_slot.clone(),
                preview_limit: lock.searchable_tools_preview_limit.clone(),
            };
            lock.tool_registry.register(search_tool);
        }
    }

    pub fn set_searchable_tools_preview_limit(&self, limit: usize) {
        let preview_limit = self.0.borrow().searchable_tools_preview_limit.clone();
        *preview_limit.lock().unwrap() = limit;
    }

    pub fn allow_tool(&self, name: &str) {
        self.0.borrow_mut().tool_registry.allow_tool(name);
    }

    /// Clone the current tool registry. Used to hand the registered tools to the
    /// Python sandbox so they can be invoked from scripts.
    pub fn registry_snapshot(&self) -> ToolRegistry {
        self.0.borrow().tool_registry.clone()
    }

    pub fn set_config(&self, config: Arc<AgentConfig>) {
        self.0.borrow_mut().config = config;
    }

    pub fn set_model(&self, model: String) {
        self.0.borrow_mut().model = model;
    }

    pub fn model(&self) -> String {
        self.0.borrow().model.clone()
    }

    pub fn model_registry(&self) -> ModelRegistry {
        self.0.borrow().config.model_registry.clone()
    }

    pub fn set_thread(&self, system_prompt: String, thread: Vec<Message>) {
        let mut lock = self.0.borrow_mut();
        lock.thread = thread_with_system_prompt(system_prompt, thread);
        lock.tool_display_names.clear();
    }

    pub fn thread(&self) -> Vec<Message> {
        self.0.borrow().thread.clone()
    }

    pub fn tool_display_names(&self) -> HashMap<usize, String> {
        self.0.borrow().tool_display_names.clone()
    }

    pub fn tool_definitions(&self) -> Vec<llm::Tool> {
        self.0.borrow().tool_registry.definitions()
    }

    #[cfg(test)]
    async fn make_inner_turn(
        &self,
        request: String,
        images: Vec<ImageSource>,
    ) -> AgentResponseStream {
        self.make_turn_with_context(request, images, None).await
    }

    async fn make_turn_with_context(
        &self,
        request: String,
        images: Vec<ImageSource>,
        turn_context: Option<TurnContext>,
    ) -> AgentResponseStream {
        let (config, model, model_key, provider, max_iterations) = {
            let lock = self.0.borrow();
            let requested = lock.model.clone();
            let model_key = if lock.config.model_registry.get(&requested).is_some() {
                requested
            } else {
                DEFAULT_MODEL.to_string()
            };
            (
                lock.config.clone(),
                lock.config
                    .model_registry
                    .get_or_default(&model_key)
                    .clone(),
                model_key.clone(),
                lock.config
                    .model_registry
                    .provider(&model_key)
                    .unwrap_or("unknown")
                    .to_string(),
                lock.config.max_iterations,
            )
        };

        self.0.borrow_mut().thread.push(Message {
            role: llm::Role::User,
            content: request,
            images: (!images.is_empty()).then_some(images),
            ..Default::default()
        });

        let agent = self.0.clone();

        Box::pin(stream! {
            for _ in 0..max_iterations {
                let tools = {
                    let lock = agent.borrow();
                    let mut t = lock.tool_registry.definitions();
                    let extra: Vec<_> = lock.search_slot.lock().unwrap().drain(..).collect();
                    t.extend(extra);
                    t
                };

                {
                    agent.borrow_mut().thread.push(Message {
                        role: llm::Role::Assistant,
                        content: String::new(),
                        ..Default::default()
                    });
                }

                let request = {
                    let lock = agent.borrow();
                    let mut request = CompletionRequest {
                        model: model.model_name.clone(),
                        messages: lock.thread.clone(),
                        stream: Some(true),
                        tools: (!tools.is_empty()).then_some(tools.clone()),
                        ..Default::default()
                    };
                    request.reasoning_effort = model.reasoning_effort;
                    request
                };

                let mut stream = model.api.stream_completion(&model.token, request);
                let mut tool_calls = BTreeMap::new();

                while let Some(chunk) = stream.next().await {
                    let is_err = chunk.is_err();
                    if let Ok(ref chunk) = chunk {
                        if let Some(usage) = &chunk.usage {
                            config.usage_observer.token_usage(TokenUsage {
                                input_tokens: usage.prompt_tokens as u64,
                                output_tokens: usage.completion_tokens as u64,
                                model: model_key.clone(),
                                provider: provider.clone(),
                            });
                        }
                        append_chunk(&agent, chunk, &mut tool_calls);
                    }
                    match chunk {
                        Ok(chunk) => { yield Ok(AgentResponseChunk::Chunk(chunk)); },
                        Err(err) => { yield Err(AgentError::from(err)); },
                    }
                    if is_err {
                        return;
                    }
                }

                let tool_calls = finish_tool_calls(&agent, tool_calls);
                if tool_calls.is_empty() {
                    return;
                }

                let mut pending_calls = VecDeque::new();
                for (index, (id, name, arguments)) in tool_calls.into_iter().enumerate() {
                    tracing::info!(tool = %name, arguments = %arguments, "tool call requested");

                    let (tool, needs_approval) = {
                        let lock = agent.borrow();
                        let tool = lock.tool_registry.get(&name);
                        let needs_approval = serde_json::from_str::<Value>(&arguments)
                            .ok()
                            .is_some_and(|args| lock.tool_registry.needs_approval(&name, &args));
                        (tool, needs_approval)
                    };
                    let readable_name = tool
                        .as_ref()
                        .map(|tool| tool.readable_name().to_string())
                        .unwrap_or_else(|| name.clone());

                    yield Ok(AgentResponseChunk::ToolStarted {
                        tool_call_id: id.clone(),
                        name: name.clone(),
                        arguments: arguments.clone(),
                    });
                    let context = turn_context.clone().map(|context| {
                        ToolContext::new(context, id.clone(), config.id_gen.clone())
                    });
                    pending_calls.push_back(ToolExecutionRequest {
                        index,
                        id,
                        name,
                        arguments,
                        readable_name,
                        tool,
                        needs_approval,
                        context,
                    });
                }

                let (interaction_tx, mut interaction_rx) = mpsc::unbounded();
                let mut executions = FuturesUnordered::new();
                let concurrency = config.max_concurrent_tools.max(1);
                for _ in 0..concurrency {
                    if let Some(call) = pending_calls.pop_front() {
                        executions.push(execute_tool_call(config.clone(), call, interaction_tx.clone()));
                    }
                }
                let mut completed = Vec::new();
                while !executions.is_empty() {
                    match futures::future::select(executions.next(), interaction_rx.next()).await {
                        Either::Left((Some(execution), _)) => {
                            yield Ok(AgentResponseChunk::ToolFinished {
                                tool_call_id: execution.id.clone(),
                                name: execution.name.clone(),
                                result: execution.content.clone(),
                            });
                            completed.push(execution);
                            if let Some(call) = pending_calls.pop_front() {
                                executions.push(execute_tool_call(config.clone(), call, interaction_tx.clone()));
                            }
                        }
                        Either::Right((Some(interaction), _)) => yield Ok(interaction),
                        _ => break,
                    }
                }

                completed.sort_by_key(|execution| execution.index);
                let mut pending_images = Vec::new();
                for execution in completed {
                    pending_images.extend(execution.images);
                    append_tool_content(
                        &agent,
                        execution.id,
                        execution.content,
                        execution.readable_name,
                    );
                }

                // Images produced by tools this turn are surfaced as a single
                // user message after every tool result is committed, keeping the
                // assistant/tool-call ordering valid for the provider API.
                if !pending_images.is_empty() {
                    agent.borrow_mut().thread.push(Message {
                        role: llm::Role::User,
                        content: "Attached image(s) for you to view.".to_string(),
                        images: Some(pending_images),
                        ..Default::default()
                    });
                }
            }

            yield Err(AgentError::NetworkError(format!(
                "reached maximum tool iteration limit ({max_iterations})"
            )));
        })
    }

    pub async fn make_turn(
        &self,
        request: String,
        images: Vec<ImageSource>,
        context: TurnContext,
    ) -> ThreadEventStream {
        let id_gen = self.0.borrow().config.id_gen.clone();
        let user_message_id = id_gen.new_uuid_v7();
        let (contextual_tx, mut contextual_rx) = mpsc::unbounded();
        let inner_context = context.clone().with_sink(Arc::new(TeeEventSink {
            sink: context.sink.clone(),
            sender: contextual_tx,
        }));
        let mut source = self
            .make_turn_with_context(request, images, Some(inner_context))
            .await;

        Box::pin(stream! {
            macro_rules! emit {
                ($kind:expr) => {{
                    let event = ThreadEvent {
                        id: id_gen.new_uuid_v7(),
                        run_id: context.run_id,
                        agent_path: context.agent_path.clone(),
                        kind: $kind,
                    };
                    context.sink.emit(event.clone());
                    yield event;
                }};
            }

            emit!(EventKind::RunStarted);
            if context.emit_user_message_events {
                emit!(EventKind::MessageStarted {
                    message_id: user_message_id,
                    role: MessageRole::User,
                });
                emit!(EventKind::MessageCommitted {
                    message_id: user_message_id,
                });
            }
            let mut current_tool_call_id = String::new();
            let mut assistant_message_id = None;
            loop {
                let item = match futures::future::select(source.next(), contextual_rx.next()).await {
                    Either::Left((Some(item), _)) => item,
                    Either::Right((Some(event), _)) => {
                        yield event;
                        continue;
                    }
                    _ => break,
                };
                match item {
                    Ok(AgentResponseChunk::Chunk(chunk)) => {
                        let message_id = match assistant_message_id {
                            Some(message_id) => message_id,
                            None => {
                                let message_id = id_gen.new_uuid_v7();
                                assistant_message_id = Some(message_id);
                                emit!(EventKind::MessageStarted {
                                    message_id,
                                    role: MessageRole::Assistant,
                                });
                                message_id
                            }
                        };
                        let finishes_message = chunk
                            .choices
                            .iter()
                            .any(|choice| choice.finish_reason.is_some());
                        for choice in chunk.choices {
                            if let Some(delta) = choice.delta {
                                if let Some(content) = delta.content.filter(|content| !content.is_empty()) {
                                    emit!(EventKind::TextDelta {
                                        message_id,
                                        delta: content,
                                    });
                                }
                                if let Some(thinking) = delta.thinking.filter(|thinking| !thinking.is_empty()) {
                                    emit!(EventKind::ThinkingDelta {
                                        message_id,
                                        delta: thinking,
                                    });
                                }
                            } else if let Some(message) = choice.message {
                                if !message.content.is_empty() {
                                    emit!(EventKind::TextDelta {
                                        message_id,
                                        delta: message.content,
                                    });
                                }
                                if let Some(thinking) = message.thinking.filter(|thinking| !thinking.is_empty()) {
                                    emit!(EventKind::ThinkingDelta {
                                        message_id,
                                        delta: thinking,
                                    });
                                }
                            } else if let Some(text) = choice.text.filter(|text| !text.is_empty()) {
                                emit!(EventKind::TextDelta {
                                    message_id,
                                    delta: text,
                                });
                            }
                        }
                        if finishes_message {
                            emit!(EventKind::MessageCommitted { message_id });
                            assistant_message_id = None;
                        }
                    }
                    Ok(AgentResponseChunk::ToolStarted {
                        tool_call_id,
                        name,
                        arguments,
                        ..
                    }) => {
                        current_tool_call_id.clone_from(&tool_call_id);
                        emit!(EventKind::ToolCallStarted {
                            tool_call_id,
                            name,
                            arguments,
                        });
                    }
                    Ok(AgentResponseChunk::ToolFinished {
                        tool_call_id,
                        name,
                        result,
                        ..
                    }) => {
                        let is_error = serde_json::from_str::<Value>(&result)
                            .is_ok_and(|value| value.get("error").is_some());
                        emit!(EventKind::ToolCallFinished {
                            tool_call_id,
                            name,
                            result,
                            is_error,
                        });
                    }
                    Ok(AgentResponseChunk::Approval { message, approved, .. }) => {
                        let approval_id = id_gen.new_uuid_v7();
                        let response = context.broker.request_approval(
                            approval_id,
                            current_tool_call_id.clone(),
                            message.clone(),
                        );
                        emit!(EventKind::ApprovalRequested {
                            approval_id,
                            tool_call_id: current_tool_call_id.clone(),
                            message,
                        });
                        let approved_value = response.await;
                        let _ = approved.send(approved_value);
                        emit!(EventKind::ApprovalResolved {
                            approval_id,
                            approved: approved_value,
                        });
                    }
                    Ok(AgentResponseChunk::Quiz { questions, answered, .. }) => {
                        let quiz_id = id_gen.new_uuid_v7();
                        let response = context.broker.request_quiz(quiz_id, questions.clone());
                        emit!(EventKind::QuizRequested {
                            quiz_id,
                            questions,
                        });
                        let answers = response.await;
                        let _ = answered.send(answers);
                        emit!(EventKind::QuizAnswered { quiz_id });
                    }
                    Err(error) => {
                        emit!(EventKind::RunFailed {
                            error: error.to_string(),
                        });
                        return;
                    }
                }
            }

            if let Some(message_id) = assistant_message_id {
                emit!(EventKind::MessageCommitted { message_id });
            }
            emit!(EventKind::RunFinished);
        })
    }
}

struct ToolExecutionRequest {
    index: usize,
    id: String,
    name: String,
    arguments: String,
    readable_name: String,
    tool: Option<Arc<dyn Tool>>,
    needs_approval: bool,
    context: Option<ToolContext>,
}

struct ToolExecution {
    index: usize,
    id: String,
    name: String,
    readable_name: String,
    content: String,
    images: Vec<ImageSource>,
}

async fn execute_tool_call(
    config: Arc<AgentConfig>,
    call: ToolExecutionRequest,
    interaction_tx: mpsc::UnboundedSender<AgentResponseChunk>,
) -> ToolExecution {
    let result = match serde_json::from_str::<Value>(&call.arguments) {
        Ok(args) => match call.tool {
            Some(tool) => {
                if let Some(questions) = tool.quiz_questions(&args) {
                    let answers = if let Some(context) = &call.context {
                        context.request_quiz(questions.clone()).await
                    } else {
                        let (answered, response) = oneshot::channel();
                        let _ = interaction_tx.unbounded_send(AgentResponseChunk::Quiz {
                            questions: questions.clone(),
                            answered,
                        });
                        response.await.unwrap_or_default()
                    };
                    json!({
                        "answers": questions.iter().zip(answers.iter()).map(|(question, answer)| {
                            json!({ "question": question.question, "answer": answer })
                        }).collect::<Vec<_>>()
                    })
                } else if call.needs_approval {
                    let message = tool.confirmation_prompt(&args);
                    let approved = if let Some(context) = &call.context {
                        context.request_approval(message).await
                    } else {
                        let (approved, response) = oneshot::channel();
                        let _ = interaction_tx
                            .unbounded_send(AgentResponseChunk::Approval { message, approved });
                        response.await.unwrap_or(false)
                    };
                    if approved {
                        tool.execute_with_context(config, args, call.context).await
                    } else {
                        json!({ "error": "tool execution denied by user" })
                    }
                } else {
                    tool.execute_with_context(config, args, call.context).await
                }
            }
            None => json!({ "error": format!("unknown tool: {}", call.name) }),
        },
        Err(error) => json!({ "error": error.to_string() }),
    };

    if result.get("error").is_some() {
        log_tool_error(&call.name, &result);
    } else {
        log_tool_result(&call.name, &result);
    }
    let mut result = result;
    let images = take_tool_images(&mut result);
    let content =
        serde_json::to_string(&result).unwrap_or_else(|error| format!(r#"{{"error":"{error}"}}"#));

    ToolExecution {
        index: call.index,
        id: call.id,
        name: call.name,
        readable_name: call.readable_name,
        content,
        images,
    }
}

fn log_tool_error(name: &str, result: &Value) {
    if let Some(error) = result.get("error") {
        tracing::error!(tool = %name, error = %error, "tool call failed");
    }
}

fn log_tool_result(name: &str, result: &Value) {
    if result.get("error").is_some() {
        log_tool_error(name, result);
    } else {
        tracing::info!(tool = %name, "tool call finished");
    }
}

fn thread_with_system_prompt(system_prompt: String, thread: Vec<Message>) -> Vec<Message> {
    let mut prompted_thread = Vec::with_capacity(thread.len() + 1);
    prompted_thread.push(Message {
        role: llm::Role::System,
        content: system_prompt,
        ..Default::default()
    });
    prompted_thread.extend(thread);
    prompted_thread
}

fn append_chunk(
    agent: &Rc<RefCell<BaseAgentInner>>,
    chunk: &StreamResponseChunk,
    tool_calls: &mut BTreeMap<usize, PartialToolCall>,
) {
    let mut lock = agent.borrow_mut();
    let message = lock.thread.last_mut().unwrap();

    for choice in &chunk.choices {
        if let Some(choice_message) = &choice.message {
            if !choice_message.content.is_empty() {
                message.content.push_str(&choice_message.content);
            }

            if let Some(thinking) = &choice_message.thinking
                && !thinking.is_empty()
            {
                message
                    .thinking
                    .get_or_insert_with(String::new)
                    .push_str(thinking);
            }

            if let Some(chunks) = &choice_message.tool_calls {
                append_tool_call_chunks(tool_calls, chunks);
            }
        }

        if let Some(content) = &choice.text {
            message.content.push_str(content);
        }

        if let Some(delta) = &choice.delta {
            if let Some(content) = &delta.content {
                message.content.push_str(content);
            }

            if let Some(thinking) = &delta.thinking {
                message
                    .thinking
                    .get_or_insert_with(String::new)
                    .push_str(thinking);
            }

            if let Some(chunks) = &delta.tool_calls {
                append_tool_call_chunks(tool_calls, chunks);
            }
        }
    }
}

fn append_tool_call_chunks(
    tool_calls: &mut BTreeMap<usize, PartialToolCall>,
    chunks: &[ToolCallChunk],
) {
    for chunk in chunks {
        let index = chunk.index.unwrap_or(0);
        let call = tool_calls.entry(index).or_default();

        if let Some(id) = &chunk.id {
            call.id.push_str(id);
        }

        if let Some(function) = &chunk.function {
            if let Some(name) = &function.name {
                call.name.push_str(name);
            }
            if let Some(arguments) = &function.arguments {
                call.arguments.push_str(arguments);
            }
        }
    }
}

fn finish_tool_calls(
    agent: &Rc<RefCell<BaseAgentInner>>,
    tool_calls: BTreeMap<usize, PartialToolCall>,
) -> Vec<(String, String, String)> {
    let tool_calls: Vec<_> = tool_calls
        .into_values()
        .filter(|call| !call.name.is_empty())
        .map(|call| (call.id, call.name, call.arguments))
        .collect();

    if tool_calls.is_empty() {
        return tool_calls;
    }

    let mut lock = agent.borrow_mut();
    let message = lock.thread.last_mut().unwrap();
    message.tool_calls = Some(
        tool_calls
            .iter()
            .map(|(id, name, arguments)| ToolCallChunk {
                index: None,
                id: Some(id.clone()),
                call_type: Some("function".to_string()),
                function: Some(ToolCallFunction {
                    name: Some(name.clone()),
                    arguments: Some(arguments.clone()),
                }),
            })
            .collect(),
    );

    tool_calls
}

/// Removes the `__images` convention key from a tool result, returning any
/// images the tool wants shown to the model. They are surfaced as a follow-up
/// user message after all tool results for the turn are committed.
fn take_tool_images(result: &mut Value) -> Vec<ImageSource> {
    let Some(object) = result.as_object_mut() else {
        return Vec::new();
    };
    let Some(images) = object.remove("__images") else {
        return Vec::new();
    };
    serde_json::from_value(images).unwrap_or_default()
}

fn append_tool_content(
    agent: &Rc<RefCell<BaseAgentInner>>,
    id: String,
    content: String,
    readable_name: String,
) {
    let mut lock = agent.borrow_mut();
    lock.thread.push(Message {
        role: llm::Role::Tool,
        content,
        tool_call_id: Some(id),
        ..Default::default()
    });
    let index = lock.thread.len() - 1;
    lock.tool_display_names.insert(index, readable_name);
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    use async_trait::async_trait;
    use futures::{StreamExt, pin_mut};
    use llm::{
        CompletionChoice, Delta, Function, FunctionParameters, StreamResponseChunk, ToolCallChunk,
        ToolCallFunction,
    };

    use super::*;
    use crate::InteractionBroker;

    #[derive(Clone)]
    struct ApprovalTool {
        calls: Arc<AtomicUsize>,
    }

    struct GatedTool {
        name: String,
        gate: Mutex<Option<oneshot::Receiver<()>>>,
    }

    #[derive(Default)]
    struct RecordingSink(Mutex<Vec<ThreadEvent>>);

    impl crate::EventSink for RecordingSink {
        fn emit(&self, event: ThreadEvent) {
            self.0.lock().unwrap().push(event);
        }
    }

    #[async_trait(?Send)]
    impl Tool for GatedTool {
        fn name(&self) -> &str {
            &self.name
        }

        fn readable_name(&self) -> &str {
            &self.name
        }

        fn definition(&self) -> llm::Tool {
            llm::Tool {
                r#type: llm::ToolType::Function,
                function: Function {
                    description: "Wait for a test gate".to_owned(),
                    name: self.name.clone(),
                    parameters: Some(FunctionParameters {
                        param_type: "object".to_owned(),
                        ..Default::default()
                    }),
                },
            }
        }

        async fn execute(&self, _config: Arc<AgentConfig>, _args: Value) -> Value {
            let receiver = self.gate.lock().unwrap().take().unwrap();
            let _ = receiver.await;
            json!({ "tool": self.name })
        }
    }

    #[async_trait(?Send)]
    impl Tool for ApprovalTool {
        fn name(&self) -> &str {
            "approval_tool"
        }

        fn readable_name(&self) -> &str {
            "Approval tool"
        }

        fn definition(&self) -> llm::Tool {
            llm::Tool {
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

        fn confirmation_prompt(&self, args: &Value) -> String {
            format!("Approve approval_tool with {args}")
        }
    }

    fn make_agent(mock: &llm::Mock, calls: Arc<AtomicUsize>) -> BaseAgent {
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
        let agent = BaseAgent::new(
            DEFAULT_MODEL.to_string(),
            Arc::new(AgentConfig {
                model_registry: registry,
                max_iterations: 50,
                usage_observer: Arc::new(stride_agent::NoopUsageObserver),
                ..Default::default()
            }),
            String::new(),
            vec![],
        );
        agent.register_tool(ApprovalTool { calls });
        agent
    }

    #[test]
    fn take_tool_images_strips_and_returns_images() {
        let mut result = json!({
            "success": true,
            "__images": [{ "url": "https://example.com/a.png" }],
        });
        let images = take_tool_images(&mut result);
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].url.as_deref(), Some("https://example.com/a.png"));
        assert!(result.get("__images").is_none());
        assert_eq!(result["success"], true);
    }

    #[test]
    fn make_turn_attaches_images_to_user_message() {
        futures::executor::block_on(async {
            let mock = llm::Mock::new().with_stream_chunks(vec![vec![text_chunk("done")]]);
            let agent = BaseAgent::new(
                DEFAULT_MODEL.to_string(),
                Arc::new(AgentConfig {
                    model_registry: registry(&mock),
                    max_iterations: 50,
                    usage_observer: Arc::new(stride_agent::NoopUsageObserver),
                    ..Default::default()
                }),
                String::new(),
                vec![],
            );

            let stream = agent
                .make_inner_turn(
                    "look".to_string(),
                    vec![ImageSource::url("https://example.com/a.png")],
                )
                .await;
            pin_mut!(stream);
            while stream.next().await.is_some() {}

            let requests = mock.stream_requests();
            let user = requests[0]
                .messages
                .iter()
                .find(|message| message.role == llm::Role::User)
                .unwrap();
            let images = user.images.as_ref().unwrap();
            assert_eq!(images[0].url.as_deref(), Some("https://example.com/a.png"));
        });
    }

    #[test]
    fn sends_system_prompt_before_thread_messages() {
        futures::executor::block_on(async {
            let calls = Arc::new(AtomicUsize::new(0));
            let mock = llm::Mock::new().with_stream_chunks(vec![vec![text_chunk("done")]]);
            let agent = BaseAgent::new(
                DEFAULT_MODEL.to_string(),
                Arc::new(AgentConfig {
                    model_registry: registry(&mock),
                    max_iterations: 50,
                    usage_observer: Arc::new(stride_agent::NoopUsageObserver),
                    ..Default::default()
                }),
                "Use short answers.".to_string(),
                vec![Message {
                    role: llm::Role::User,
                    content: "previous".to_string(),
                    ..Default::default()
                }],
            );
            agent.register_tool(ApprovalTool { calls });

            let stream = agent.make_inner_turn("next".to_string(), vec![]).await;
            pin_mut!(stream);
            while stream.next().await.is_some() {}

            let requests = mock.stream_requests();
            assert_eq!(requests[0].messages[0].role, llm::Role::System);
            assert_eq!(requests[0].messages[0].content, "Use short answers.");
            assert_eq!(requests[0].messages[1].content, "previous");
            assert_eq!(requests[0].messages[2].content, "next");
        });
    }

    #[test]
    fn waits_for_approval_before_executing_tool() {
        futures::executor::block_on(async {
            let calls = Arc::new(AtomicUsize::new(0));
            let mock = llm::Mock::new().with_stream_chunks(vec![
                vec![tool_call_chunk(r#"{"value":1}"#)],
                vec![text_chunk("done")],
            ]);
            let agent = make_agent(&mock, calls.clone());

            let stream = agent.make_inner_turn("run tool".to_string(), vec![]).await;
            pin_mut!(stream);

            assert!(matches!(
                stream.next().await.unwrap().unwrap(),
                AgentResponseChunk::Chunk(_)
            ));
            assert!(matches!(
                stream.next().await.unwrap().unwrap(),
                AgentResponseChunk::ToolStarted { .. }
            ));

            match stream.next().await.unwrap().unwrap() {
                AgentResponseChunk::Approval {
                    message, approved, ..
                } => {
                    assert_eq!(message, r#"Approve approval_tool with {"value":1}"#);
                    approved.send(true).unwrap();
                }
                AgentResponseChunk::Chunk(_)
                | AgentResponseChunk::Quiz { .. }
                | AgentResponseChunk::ToolStarted { .. }
                | AgentResponseChunk::ToolFinished { .. } => {
                    panic!("expected approval")
                }
            }

            assert_eq!(calls.load(Ordering::SeqCst), 0);

            let mut saw_done = false;
            while let Some(chunk) = stream.next().await {
                if chunk_text(&chunk.unwrap()) == Some("done") {
                    saw_done = true;
                }
            }

            assert!(saw_done);
            assert_eq!(calls.load(Ordering::SeqCst), 1);
            assert!(
                agent
                    .tool_display_names()
                    .values()
                    .any(|name| name == "Approval tool")
            );
            assert_eq!(mock.stream_requests().len(), 2);
        });
    }

    #[test]
    fn denial_skips_tool_execution_and_reports_result() {
        futures::executor::block_on(async {
            let calls = Arc::new(AtomicUsize::new(0));
            let mock = llm::Mock::new().with_stream_chunks(vec![
                vec![tool_call_chunk(r#"{"value":1}"#)],
                vec![text_chunk("done")],
            ]);
            let agent = make_agent(&mock, calls.clone());

            let stream = agent.make_inner_turn("run tool".to_string(), vec![]).await;
            pin_mut!(stream);

            stream.next().await.unwrap().unwrap();
            stream.next().await.unwrap().unwrap();

            match stream.next().await.unwrap().unwrap() {
                AgentResponseChunk::Approval { approved, .. } => approved.send(false).unwrap(),
                AgentResponseChunk::Chunk(_)
                | AgentResponseChunk::Quiz { .. }
                | AgentResponseChunk::ToolStarted { .. }
                | AgentResponseChunk::ToolFinished { .. } => {
                    panic!("expected approval")
                }
            }

            while stream.next().await.is_some() {}

            assert_eq!(calls.load(Ordering::SeqCst), 0);

            let requests = mock.stream_requests();
            let tool_result = requests[1]
                .messages
                .iter()
                .find(|message| message.role == llm::Role::Tool)
                .unwrap();
            assert_eq!(
                tool_result.content,
                r#"{"error":"tool execution denied by user"}"#
            );
        });
    }

    #[test]
    fn tools_finish_concurrently_but_enter_transcript_in_call_order() {
        futures::executor::block_on(async {
            let mock = llm::Mock::new()
                .with_stream_chunks(vec![vec![two_tool_calls_chunk()], vec![text_chunk("done")]]);
            let (first_tx, first_rx) = oneshot::channel();
            let (second_tx, second_rx) = oneshot::channel();
            let agent = BaseAgent::new(
                DEFAULT_MODEL.to_owned(),
                Arc::new(AgentConfig {
                    model_registry: registry(&mock),
                    max_iterations: 4,
                    ..Default::default()
                }),
                String::new(),
                vec![],
            );
            agent.register_tool(GatedTool {
                name: "first".to_owned(),
                gate: Mutex::new(Some(first_rx)),
            });
            agent.register_tool(GatedTool {
                name: "second".to_owned(),
                gate: Mutex::new(Some(second_rx)),
            });

            let mut stream = agent.make_inner_turn("run both".to_owned(), vec![]).await;
            assert!(matches!(
                stream.next().await.unwrap().unwrap(),
                AgentResponseChunk::Chunk(_)
            ));
            assert!(
                matches!(stream.next().await.unwrap().unwrap(), AgentResponseChunk::ToolStarted { name, .. } if name == "first")
            );
            assert!(
                matches!(stream.next().await.unwrap().unwrap(), AgentResponseChunk::ToolStarted { name, .. } if name == "second")
            );

            second_tx.send(()).unwrap();
            assert!(
                matches!(stream.next().await.unwrap().unwrap(), AgentResponseChunk::ToolFinished { name, .. } if name == "second")
            );
            first_tx.send(()).unwrap();
            assert!(
                matches!(stream.next().await.unwrap().unwrap(), AgentResponseChunk::ToolFinished { name, .. } if name == "first")
            );
            while stream.next().await.is_some() {}

            let requests = mock.stream_requests();
            let tool_results = requests[1]
                .messages
                .iter()
                .filter(|message| message.role == llm::Role::Tool)
                .map(|message| message.tool_call_id.as_deref().unwrap())
                .collect::<Vec<_>>();
            assert_eq!(tool_results, vec!["call_1", "call_2"]);
        });
    }

    #[test]
    fn event_turn_emits_addressed_serializable_events() {
        futures::executor::block_on(async {
            let mock = llm::Mock::new().with_stream_chunks(vec![vec![text_chunk("hello")]]);
            let agent = BaseAgent::new(
                DEFAULT_MODEL.to_owned(),
                Arc::new(AgentConfig {
                    model_registry: registry(&mock),
                    max_iterations: 4,
                    id_gen: Arc::new(crate::SeededIdGen::new(11)),
                    ..Default::default()
                }),
                String::new(),
                vec![],
            );
            let context = TurnContext::new(
                uuid::Uuid::from_u128(99),
                Arc::new(crate::NoopEventSink),
                Arc::new(crate::InMemoryInteractionBroker::default()),
            );

            let events = agent
                .make_turn("hi".to_owned(), vec![], context)
                .await
                .collect::<Vec<_>>()
                .await;

            let message_id = events
                .iter()
                .find_map(|event| match event.kind {
                    EventKind::MessageStarted {
                        message_id,
                        role: MessageRole::Assistant,
                    } => Some(message_id),
                    _ => None,
                })
                .unwrap();
            assert!(events.iter().any(|event| matches!(
                &event.kind,
                EventKind::TextDelta { message_id: id, delta }
                    if *id == message_id && delta == "hello"
            )));
            assert!(events.iter().any(|event| matches!(
                event.kind,
                EventKind::MessageCommitted { message_id: id } if id == message_id
            )));
            assert!(serde_json::to_string(&events).is_ok());
            insta::assert_json_snapshot!(
                "text_turn_events",
                events.iter().map(|event| &event.kind).collect::<Vec<_>>()
            );
        });
    }

    #[test]
    fn event_turn_registers_multiple_approvals_concurrently() {
        futures::executor::block_on(async {
            let calls = Arc::new(AtomicUsize::new(0));
            let mock = llm::Mock::new().with_stream_chunks(vec![
                vec![two_approval_tool_calls_chunk()],
                vec![text_chunk("done")],
            ]);
            let agent = make_agent(&mock, calls.clone());
            let sink = Arc::new(RecordingSink::default());
            let broker = Arc::new(crate::InMemoryInteractionBroker::default());
            let context = TurnContext::new(uuid::Uuid::from_u128(9), sink.clone(), broker.clone());
            let mut stream = agent
                .make_turn("run both".to_owned(), vec![], context)
                .await;

            let mut started = 0;
            let mut approval_ids = Vec::new();
            while started < 2 || approval_ids.len() < 2 {
                let event = stream.next().await.unwrap();
                match event.kind {
                    EventKind::ToolCallStarted { .. } => started += 1,
                    EventKind::ApprovalRequested { approval_id, .. } => {
                        approval_ids.push(approval_id);
                    }
                    _ => {}
                }
            }
            let sink_approval_ids = sink
                .0
                .lock()
                .unwrap()
                .iter()
                .filter_map(|event| match event.kind {
                    EventKind::ApprovalRequested { approval_id, .. } => Some(approval_id),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(sink_approval_ids, approval_ids);
            for id in approval_ids {
                assert!(broker.resolve_approval(id, true));
            }
            while stream.next().await.is_some() {}
            assert_eq!(calls.load(Ordering::SeqCst), 2);
        });
    }

    fn two_tool_calls_chunk() -> StreamResponseChunk {
        let call = |index, id: &str, name: &str| ToolCallChunk {
            index: Some(index),
            id: Some(id.to_owned()),
            call_type: None,
            function: Some(ToolCallFunction {
                name: Some(name.to_owned()),
                arguments: Some("{}".to_owned()),
            }),
        };
        StreamResponseChunk {
            choices: vec![CompletionChoice {
                index: 0,
                delta: Some(Delta {
                    tool_calls: Some(vec![
                        call(0, "call_1", "first"),
                        call(1, "call_2", "second"),
                    ]),
                    ..Default::default()
                }),
                finish_reason: Some("tool_calls".to_owned()),
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    fn two_approval_tool_calls_chunk() -> StreamResponseChunk {
        let call = |index, id: &str| ToolCallChunk {
            index: Some(index),
            id: Some(id.to_owned()),
            call_type: None,
            function: Some(ToolCallFunction {
                name: Some("approval_tool".to_owned()),
                arguments: Some("{}".to_owned()),
            }),
        };
        StreamResponseChunk {
            choices: vec![CompletionChoice {
                index: 0,
                delta: Some(Delta {
                    tool_calls: Some(vec![call(0, "call_1"), call(1, "call_2")]),
                    ..Default::default()
                }),
                finish_reason: Some("tool_calls".to_owned()),
                ..Default::default()
            }],
            ..Default::default()
        }
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

    fn chunk_text(chunk: &AgentResponseChunk) -> Option<&str> {
        let AgentResponseChunk::Chunk(chunk) = chunk else {
            return None;
        };

        chunk
            .choices
            .first()
            .and_then(|choice| choice.delta.as_ref())
            .and_then(|delta| delta.content.as_deref())
    }

    fn registry(mock: &llm::Mock) -> ModelRegistry {
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
        registry
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

    struct EchoTool;

    #[async_trait(?Send)]
    impl Tool for EchoTool {
        fn name(&self) -> &str {
            "echo"
        }

        fn readable_name(&self) -> &str {
            "Echo"
        }

        fn definition(&self) -> llm::Tool {
            llm::Tool {
                r#type: llm::ToolType::Function,
                function: Function {
                    description: "Echo back the input message.".to_string(),
                    name: self.name().to_string(),
                    parameters: Some(FunctionParameters {
                        param_type: "object".to_string(),
                        ..Default::default()
                    }),
                },
            }
        }

        async fn execute(&self, _config: Arc<AgentConfig>, args: Value) -> Value {
            json!({ "echoed": args })
        }
    }

    struct NamedSearchableTool {
        name: &'static str,
        category: Option<&'static str>,
    }

    #[async_trait(?Send)]
    impl Tool for NamedSearchableTool {
        fn name(&self) -> &str {
            self.name
        }

        fn readable_name(&self) -> &str {
            self.name
        }

        fn definition(&self) -> llm::Tool {
            llm::Tool {
                r#type: llm::ToolType::Function,
                function: Function {
                    description: format!("Searchable test tool {}", self.name),
                    name: self.name().to_string(),
                    parameters: Some(FunctionParameters {
                        param_type: "object".to_string(),
                        ..Default::default()
                    }),
                },
            }
        }

        fn searchable_category(&self) -> Option<String> {
            self.category.map(str::to_string)
        }

        async fn execute(&self, _config: Arc<AgentConfig>, args: Value) -> Value {
            json!({ "args": args })
        }
    }

    #[test]
    fn search_tools_description_summarizes_hidden_tools() {
        let mock = llm::Mock::new();
        let agent = BaseAgent::new(
            DEFAULT_MODEL.to_string(),
            Arc::new(AgentConfig {
                model_registry: registry(&mock),
                max_iterations: 50,
                usage_observer: Arc::new(stride_agent::NoopUsageObserver),
                ..Default::default()
            }),
            String::new(),
            vec![],
        );
        agent.set_searchable_tools_preview_limit(2);
        agent.register_searchable_tool(NamedSearchableTool {
            name: "alpha",
            category: None,
        });
        agent.register_searchable_tool(NamedSearchableTool {
            name: "beta",
            category: None,
        });
        agent.register_searchable_tool(NamedSearchableTool {
            name: "gamma",
            category: None,
        });
        agent.register_searchable_tool(NamedSearchableTool {
            name: "github_create_issue",
            category: Some("GitHub MCP"),
        });
        agent.register_searchable_tool(NamedSearchableTool {
            name: "github_list_issues",
            category: Some("GitHub MCP"),
        });

        let description = agent
            .tool_definitions()
            .into_iter()
            .find(|tool| tool.function.name == "search_tools")
            .unwrap()
            .function
            .description;

        assert!(description.contains("5 additional tools"));
        assert!(description.contains("Categories include: GitHub MCP (2 tools), NONE (3 tools)."));
        assert!(!description.contains("Tool names include:"));
        assert!(!description.contains("alpha"));
        assert!(!description.contains("gamma,"));
        assert!(!description.contains("github_create_issue"));
    }

    #[test]
    fn search_tools_matches_hidden_tool_categories() {
        futures::executor::block_on(async {
            let mock = llm::Mock::new().with_stream_chunks(vec![
                vec![named_tool_call_chunk(
                    "search_tools",
                    r#"{"query":"google"}"#,
                )],
                vec![text_chunk("done")],
            ]);
            let agent = BaseAgent::new(
                DEFAULT_MODEL.to_string(),
                Arc::new(AgentConfig {
                    model_registry: registry(&mock),
                    max_iterations: 50,
                    usage_observer: Arc::new(stride_agent::NoopUsageObserver),
                    ..Default::default()
                }),
                String::new(),
                vec![],
            );
            agent.register_searchable_tool(NamedSearchableTool {
                name: "gmail_draft_reply",
                category: Some("Google"),
            });
            agent.register_searchable_tool(NamedSearchableTool {
                name: "alpha",
                category: None,
            });

            let stream = agent.make_inner_turn("go".to_string(), vec![]).await;
            pin_mut!(stream);
            let mut search_result = None;
            while let Some(chunk) = stream.next().await {
                if let AgentResponseChunk::ToolFinished { name, result, .. } = chunk.unwrap()
                    && name == "search_tools"
                {
                    search_result = Some(result);
                }
            }

            let result: Value = serde_json::from_str(&search_result.unwrap()).unwrap();
            assert_eq!(result["found"], 1);
            assert_eq!(result["tools"][0], "gmail_draft_reply");
        });
    }

    #[test]
    fn search_tools_injects_found_tools_for_one_turn() {
        futures::executor::block_on(async {
            let mock = llm::Mock::new().with_stream_chunks(vec![
                // Iteration 1: LLM calls search_tools
                vec![named_tool_call_chunk("search_tools", r#"{"query":"echo"}"#)],
                // Iteration 2: LLM uses the found echo tool
                vec![named_tool_call_chunk("echo", r#"{"msg":"hello"}"#)],
                // Iteration 3: LLM returns text
                vec![text_chunk("done")],
            ]);

            let agent = BaseAgent::new(
                DEFAULT_MODEL.to_string(),
                Arc::new(AgentConfig {
                    model_registry: registry(&mock),
                    max_iterations: 50,
                    usage_observer: Arc::new(stride_agent::NoopUsageObserver),
                    ..Default::default()
                }),
                String::new(),
                vec![],
            );
            agent.register_searchable_tool(EchoTool);

            let stream = agent.make_inner_turn("go".to_string(), vec![]).await;
            pin_mut!(stream);
            while stream.next().await.is_some() {}

            let requests = mock.stream_requests();
            assert_eq!(requests.len(), 3);

            let tool_names = |req: &llm::CompletionRequest| -> Vec<String> {
                req.tools
                    .as_deref()
                    .unwrap_or(&[])
                    .iter()
                    .map(|t| t.function.name.clone())
                    .collect()
            };

            // Iteration 1: only search_tools visible
            let names1 = tool_names(&requests[0]);
            assert!(names1.contains(&"search_tools".to_string()));
            assert!(!names1.contains(&"echo".to_string()));

            // Iteration 2: echo injected for this turn
            let names2 = tool_names(&requests[1]);
            assert!(names2.contains(&"search_tools".to_string()));
            assert!(names2.contains(&"echo".to_string()));

            // Iteration 3: echo gone again
            let names3 = tool_names(&requests[2]);
            assert!(names3.contains(&"search_tools".to_string()));
            assert!(!names3.contains(&"echo".to_string()));
        });
    }
}
