use std::collections::{BTreeMap, HashMap, HashSet};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::{cell::RefCell, rc::Rc};

use async_stream::stream;
use futures::channel::{mpsc, oneshot};
use futures::{FutureExt, Stream, StreamExt, select};
use llm::{
    API, CompletionRequest, Function, FunctionParameters, FunctionProperty, ImageSource, Message,
    OpenAI, ReasoningEffort, StreamResponseChunk, ToolCallChunk, ToolCallFunction,
};
use serde_json::json;
use serde_json::{Map, Value};
use thiserror::Error;

use crate::tools::search::{SearchEntry, SearchTool};
use crate::{QuizQuestion, TaskSpawner, Tool, ToolOutputFormat, ToolRegistry};

pub const DEFAULT_MODEL: &str = "default";
const WAIT_TOOLS_NAME: &str = "wait_tools";
const CHECK_TOOLS_NAME: &str = "check_tools";
/// Maximum bytes retained in a per-call progress tail served by `check_tools`.
const PROGRESS_TAIL_CAP: usize = 2048;

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

pub enum AgentResponseChunk {
    Chunk(StreamResponseChunk),
    ToolStarted {
        tool_call_id: String,
        name: String,
        arguments: String,
        /// True when this call is being backgrounded via the async flag. Known
        /// at emission because it is emitted after `take_async_flag`.
        background: bool,
    },
    /// Incremental human-facing output from a streaming or backgrounded tool.
    ToolProgress {
        tool_call_id: String,
        name: String,
        delta: String,
        format: ToolOutputFormat,
    },
    ToolFinished {
        tool_call_id: String,
        name: String,
        result: String,
        /// Human-facing rendering when it differs from the model-facing result
        /// (carried out of band via the `__display` result convention).
        display: Option<String>,
        format: ToolOutputFormat,
        /// Present when this finish is a backgrounded tool's completion. Carries
        /// the exact user message the loop injected into the thread so the host
        /// persists an identical message and a reloaded thread matches the live
        /// one. `None` for ordinary (synchronous) tool results.
        wake_message: Option<String>,
    },
    Approval {
        tool_name: String,
        message: String,
        approved: oneshot::Sender<bool>,
    },
    Quiz {
        tool_name: String,
        questions: Vec<QuizQuestion>,
        answered: oneshot::Sender<Vec<String>>,
    },
}

pub type AgentResponseStream =
    Pin<Box<dyn Stream<Item = Result<AgentResponseChunk, AgentError>> + 'static>>;

/// Internal event a streaming/async tool reports back to the `make_turn` loop
/// through a shared unbounded channel. `Progress` streams partial output;
/// `Finished` delivers a backgrounded tool's final result so the monitor can
/// inject it into the live thread.
enum ToolEvent {
    Progress {
        id: String,
        name: String,
        delta: String,
        format: ToolOutputFormat,
    },
    Finished {
        id: String,
        name: String,
        result: Value,
        display: Option<String>,
        format: ToolOutputFormat,
    },
}

/// Handle a streaming or backgrounded tool uses to report incremental output.
/// Cheaply cloneable; `progress` is non-blocking (unbounded send) so a tool
/// running on the same single-threaded executor never deadlocks the loop.
pub struct ToolProgressSink {
    id: String,
    name: String,
    format: ToolOutputFormat,
    tx: mpsc::UnboundedSender<ToolEvent>,
}

impl ToolProgressSink {
    pub fn progress(&self, delta: impl Into<String>) {
        let _ = self.tx.unbounded_send(ToolEvent::Progress {
            id: self.id.clone(),
            name: self.name.clone(),
            delta: delta.into(),
            format: self.format,
        });
    }
}

#[derive(Debug)]
pub struct AgentConfig {
    pub model_registry: ModelRegistry,
    pub max_iterations: usize,
}

#[derive(Clone, Debug, Default)]
pub struct ModelRegistry {
    models: HashMap<String, ModelRegEntry>,
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

struct BaseAgentInner {
    tool_registry: ToolRegistry,
    thread: Vec<Message>,
    tool_display_names: HashMap<usize, String>,
    config: Arc<AgentConfig>,
    model: String,
    search_slot: Arc<Mutex<Vec<llm::Tool>>>,
    searchable_entries: Arc<Mutex<Vec<SearchEntry>>>,
    searchable_tools_preview_limit: Arc<Mutex<usize>>,
    /// Host-provided executor for backgrounded (async) tool calls. When absent,
    /// `async` tool calls degrade to synchronous inline execution.
    spawner: Option<Rc<dyn TaskSpawner>>,
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
        self.models.insert(name.to_string(), entry);
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
            spawner: None,
        })))
    }

    /// Install the host executor used to background `async` tool calls. Without
    /// it, `async` requests run inline.
    pub fn set_spawner(&self, spawner: Rc<dyn TaskSpawner>) {
        self.0.borrow_mut().spawner = Some(spawner);
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
            group: tool.searchable_group(),
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

    // `pending_wake` is reset each iteration and set from several control-flow
    // paths, some of which continue the loop without reading it back.
    #[allow(unused_assignments)]
    pub async fn make_turn(
        &self,
        request: String,
        images: Vec<ImageSource>,
    ) -> AgentResponseStream {
        let (config, model, max_iterations) = {
            let lock = self.0.borrow();
            let model_name = &lock.model;
            (
                lock.config.clone(),
                lock.config
                    .model_registry
                    .get_or_default(model_name)
                    .clone(),
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
        let spawner = self.0.borrow().spawner.clone();

        Box::pin(stream! {
            // Channel every streaming/async tool reports into. Created once so the
            // sender survives across iterations and all spawned tasks share one
            // receiver. Unbounded keeps `progress`/`Finished` sends non-blocking,
            // which is required on a single-threaded executor.
            let (tool_tx, mut tool_rx) = mpsc::unbounded::<ToolEvent>();
            let mut outstanding_async: HashSet<String> = HashSet::new();
            let mut async_keys: HashMap<String, String> = HashMap::new();
            // Rolling tail of progress output per outstanding async call, served
            // to the model via `check_tools`. Capped and dropped on injection.
            let mut progress_buffers: HashMap<String, String> = HashMap::new();
            // Readable tool name per outstanding async call, for `check_tools`.
            let mut async_names: HashMap<String, String> = HashMap::new();
            // Set when an async result was injected since the last model request,
            // so the loop re-requests the model (the wake) even if no async work
            // remains outstanding.
            let mut pending_wake = false;
            // Completed async results wait here until a point where the thread
            // can safely take a follow-up user message — never between an
            // assistant's tool call and its tool result.
            let mut deferred: Vec<ToolEvent> = Vec::new();

            // Route a tool event: emit progress immediately (progress never
            // mutates the thread, so it is safe any time); buffer a completion
            // for injection at the next safe point.
            macro_rules! take_tool_event {
                ($ev:expr) => {
                    match $ev {
                        ToolEvent::Progress { id, name, delta, format } => {
                            if outstanding_async.contains(&id) {
                                append_progress_tail(&mut progress_buffers, &id, &delta);
                            }
                            yield Ok(AgentResponseChunk::ToolProgress {
                                tool_call_id: id,
                                name,
                                delta,
                                format,
                            });
                        }
                        finished => deferred.push(finished),
                    }
                };
            }

            // Inject every buffered async result into the thread and surface it.
            // Only valid when the last message is not an assistant awaiting tool
            // results (i.e. at turn end or after all tool results are committed).
            macro_rules! flush_deferred {
                () => {
                    for event in std::mem::take(&mut deferred) {
                        if let ToolEvent::Finished { id, name, mut result, display, format } = event {
                            outstanding_async.remove(&id);
                            async_keys.retain(|_, pending_id| pending_id != &id);
                            progress_buffers.remove(&id);
                            async_names.remove(&id);
                            pending_wake = true;
                            let display = display.or_else(|| take_tool_display(&mut result));
                            log_tool_result(&name, &result);
                            let wake_message = inject_async_result(&agent, &id, &name, &result);
                            yield Ok(AgentResponseChunk::ToolFinished {
                                tool_call_id: id,
                                name,
                                result: wake_message.clone(),
                                display,
                                format,
                                wake_message: Some(wake_message),
                            });
                        }
                    }
                };
            }

            for _ in 0..max_iterations {
                let tools = {
                    let lock = agent.borrow();
                    let mut t = lock.tool_registry.definitions();
                    let extra: Vec<_> = lock.search_slot.lock().unwrap().drain(..).collect();
                    t.extend(extra);
                    if !outstanding_async.is_empty() {
                        t.push(wait_tools_definition());
                        t.push(check_tools_definition());
                    }
                    t
                };

                // Committing to a new model request consumes any pending wake.
                pending_wake = false;

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

                let mut stream = model.api.stream_completion(&model.token, request).fuse();
                let mut tool_calls = BTreeMap::new();

                // Model-streaming phase: race model chunks against events from
                // backgrounded tools so async subagents make progress and stream
                // their output while the model is still producing this turn.
                loop {
                    select! {
                        chunk = stream.next() => {
                            let Some(chunk) = chunk else { break };
                            let is_err = chunk.is_err();
                            if let Ok(ref chunk) = chunk {
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
                        ev = tool_rx.next() => {
                            if let Some(ev) = ev {
                                take_tool_event!(ev);
                            }
                        }
                    }
                }

                let tool_calls = finish_tool_calls(&agent, tool_calls);

                if tool_calls.is_empty() {
                    // The last message is the assistant's, so any completed async
                    // results can be injected as follow-up user messages now.
                    flush_deferred!();
                    if pending_wake {
                        // An async result landed since the last request. Re-request
                        // so the model can react to it (the wake).
                        continue;
                    }
                    if outstanding_async.is_empty() {
                        return;
                    }
                    // Monitor: nothing more to say but async work is still running.
                    // Block for the next event, inject it, and loop again.
                    match tool_rx.next().await {
                        Some(ev) => take_tool_event!(ev),
                        None => return,
                    }
                    flush_deferred!();
                    continue;
                }

                let mut pending_images: Vec<ImageSource> = Vec::new();

                for (id, name, arguments) in tool_calls {
                    tracing::info!(tool = %name, arguments = %arguments, "tool call requested");

                    let tool = {
                        let lock = agent.borrow();
                        lock.tool_registry.get(&name)
                    };
                    let readable_name = tool
                        .as_ref()
                        .map(|tool| tool.readable_name().to_string())
                        .unwrap_or_else(|| name.clone());

                    let mut args = match serde_json::from_str::<Value>(&arguments) {
                        Ok(args) => args,
                        Err(err) => {
                            yield Ok(AgentResponseChunk::ToolStarted {
                                tool_call_id: id.clone(),
                                name: readable_name.clone(),
                                arguments: arguments.clone(),
                                background: false,
                            });
                            let result = json!({ "error": err.to_string() });
                            log_tool_error(&name, &result);
                            let content = append_tool_result(&agent, id.clone(), result, "tool error".to_string(), &mut pending_images);
                            yield Ok(AgentResponseChunk::ToolFinished {
                                tool_call_id: id,
                                name: readable_name.clone(),
                                result: content,
                                display: None,
                                format: ToolOutputFormat::Json,
                                wake_message: None,
                            });
                            continue;
                        }
                    };

                    let want_async = take_async_flag(&mut args);
                    let background = want_async
                        && tool.as_ref().is_some_and(|tool| tool.supports_async())
                        && spawner.is_some();

                    yield Ok(AgentResponseChunk::ToolStarted {
                        tool_call_id: id.clone(),
                        name: readable_name.clone(),
                        arguments: arguments.clone(),
                        background,
                    });

                    if name == CHECK_TOOLS_NAME {
                        while let Ok(ev) = tool_rx.try_recv() {
                            take_tool_event!(ev);
                        }
                        let result = check_tools_result(&outstanding_async, &deferred, &progress_buffers, &async_names);
                        let content = append_tool_result(&agent, id.clone(), result, CHECK_TOOLS_NAME.to_string(), &mut pending_images);
                        yield Ok(AgentResponseChunk::ToolFinished {
                            tool_call_id: id,
                            name: CHECK_TOOLS_NAME.to_string(),
                            result: content,
                            display: None,
                            format: ToolOutputFormat::Json,
                            wake_message: None,
                        });
                        continue;
                    }

                    if name == WAIT_TOOLS_NAME {
                        let target_ids = wait_tool_ids(&args, &outstanding_async);
                        while target_ids.iter().any(|target_id| {
                            outstanding_async.contains(target_id)
                                && !deferred_has_finished(&deferred, target_id)
                        }) {
                            match tool_rx.next().await {
                                Some(ev) => take_tool_event!(ev),
                                None => break,
                            }
                        }

                        let result = json!({
                            "success": true,
                            "waited_tool_call_ids": target_ids,
                            "message": "Requested async tool results have completed and were delivered as follow-up wake messages.",
                        });
                        let content = append_tool_result(&agent, id.clone(), result, WAIT_TOOLS_NAME.to_string(), &mut pending_images);
                        yield Ok(AgentResponseChunk::ToolFinished {
                            tool_call_id: id,
                            name: WAIT_TOOLS_NAME.to_string(),
                            result: content,
                            display: None,
                            format: ToolOutputFormat::Json,
                            wake_message: None,
                        });
                        flush_deferred!();
                        continue;
                    }

                    let Some(tool) = tool else {
                        let result = json!({ "error": format!("unknown tool: {}", name) });
                        log_tool_error(&name, &result);
                        let content = append_tool_result(&agent, id.clone(), result, readable_name.clone(), &mut pending_images);
                        yield Ok(AgentResponseChunk::ToolFinished {
                            tool_call_id: id,
                            name: readable_name,
                            result: content,
                            display: None,
                            format: ToolOutputFormat::Json,
                            wake_message: None,
                        });
                        continue;
                    };

                    let async_key = async_tool_key(&name, &args);
                    if let Some(existing_id) = async_keys
                        .get(&async_key)
                        .filter(|existing_id| outstanding_async.contains(*existing_id))
                    {
                        let result = async_placeholder("already_started", existing_id, &readable_name);
                        let content = append_tool_result(&agent, id.clone(), result, readable_name.clone(), &mut pending_images);
                        yield Ok(AgentResponseChunk::ToolFinished {
                            tool_call_id: id,
                            name: readable_name,
                            result: content,
                            display: None,
                            format: ToolOutputFormat::Json,
                            wake_message: None,
                        });
                        continue;
                    }

                    if let Some(questions) = tool.quiz_questions(&args) {
                        let (answered, response) = oneshot::channel();
                        yield Ok(AgentResponseChunk::Quiz {
                            tool_name: readable_name.clone(),
                            questions: questions.clone(),
                            answered,
                        });
                        let answers = response.await.unwrap_or_default();
                        let result = json!({
                            "answers": questions.iter().zip(answers.iter()).map(|(q, a)| {
                                json!({ "question": q.question, "answer": a })
                            }).collect::<Vec<_>>()
                        });
                        log_tool_result(&name, &result);
                        let content = append_tool_result(&agent, id.clone(), result, readable_name.clone(), &mut pending_images);
                        yield Ok(AgentResponseChunk::ToolFinished {
                            tool_call_id: id,
                            name: readable_name,
                            result: content,
                            display: None,
                            format: ToolOutputFormat::Json,
                            wake_message: None,
                        });
                        continue;
                    }

                    let needs_approval = {
                        let lock = agent.borrow();
                        lock.tool_registry.needs_approval(&name, &args)
                    };

                    if needs_approval {
                        let message = tool.confirmation_prompt(&args);
                        let (approved, response) = oneshot::channel();
                        yield Ok(AgentResponseChunk::Approval { tool_name: readable_name.clone(), message, approved });

                        if !response.await.unwrap_or(false) {
                            let result = json!({ "error": "tool execution denied by user" });
                            log_tool_error(&name, &result);
                            let content = append_tool_result(&agent, id.clone(), result, readable_name.clone(), &mut pending_images);
                            yield Ok(AgentResponseChunk::ToolFinished {
                                tool_call_id: id,
                                name: readable_name.clone(),
                                result: content,
                                display: None,
                                format: ToolOutputFormat::Json,
                                wake_message: None,
                            });
                            continue;
                        }
                    }

                    let format = tool.output_format();

                    // Background execution: only when the model asked for it, the
                    // tool opts in, and the host installed an executor. The
                    // placeholder result keeps the assistant tool_call API-valid;
                    // the real result is injected later by the monitor.
                    if want_async
                        && tool.supports_async()
                        && let Some(spawner) = spawner.as_ref()
                    {
                        let placeholder = async_placeholder("started", &id, &readable_name);
                        let content = append_tool_result(&agent, id.clone(), placeholder, readable_name.clone(), &mut pending_images);
                        spawn_tool_task(
                            spawner,
                            AsyncToolTask {
                                tool: tool.clone(),
                                config: config.clone(),
                                args,
                                id: id.clone(),
                                name: readable_name.clone(),
                                format,
                            },
                            tool_tx.clone(),
                        );
                        async_keys.insert(async_key, id.clone());
                        outstanding_async.insert(id.clone());
                        async_names.insert(id.clone(), readable_name.clone());
                        yield Ok(AgentResponseChunk::ToolFinished {
                            tool_call_id: id.clone(),
                            name: readable_name.clone(),
                            result: content,
                            display: None,
                            format: ToolOutputFormat::Json,
                            wake_message: None,
                        });
                        continue;
                    }

                    // Inline execution. Streaming tools drain their progress live
                    // (backgrounded completions that land meanwhile are buffered
                    // and injected after this tool's result is committed).
                    let mut result = if tool.streams() {
                        let sink = ToolProgressSink {
                            id: id.clone(),
                            name: readable_name.clone(),
                            format,
                            tx: tool_tx.clone(),
                        };
                        let mut exec = Box::pin(tool.execute_streaming(config.clone(), args, sink)).fuse();
                        let result = loop {
                            select! {
                                r = exec => break r,
                                ev = tool_rx.next() => {
                                    if let Some(ev) = ev {
                                        take_tool_event!(ev);
                                    }
                                }
                            }
                        };
                        while let Ok(ev) = tool_rx.try_recv() {
                            take_tool_event!(ev);
                        }
                        result
                    } else {
                        tool.execute(config.clone(), args).await
                    };

                    let display = take_tool_display(&mut result);
                    log_tool_result(&name, &result);
                    let content = append_tool_result(&agent, id.clone(), result, readable_name.clone(), &mut pending_images);
                    yield Ok(AgentResponseChunk::ToolFinished {
                        tool_call_id: id,
                        name: readable_name,
                        result: content,
                        display,
                        format,
                        wake_message: None,
                    });
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

                // Every tool result for this turn is committed, so it is now safe
                // to inject any async completions that landed while dispatching.
                flush_deferred!();
            }

            yield Err(AgentError::NetworkError(format!(
                "reached maximum tool iteration limit ({max_iterations})"
            )));
        })
    }
}

/// Removes the reserved `async` flag from a tool call's arguments, returning
/// whether the model requested background execution.
fn take_async_flag(args: &mut Value) -> bool {
    args.as_object_mut()
        .and_then(|object| object.remove(crate::ASYNC_ARG))
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}

fn wait_tools_definition() -> llm::Tool {
    let mut tool_call_ids_extra = Map::new();
    tool_call_ids_extra.insert("items".to_string(), json!({ "type": "string" }));

    let mut properties = HashMap::new();
    properties.insert(
        "tool_call_ids".to_string(),
        FunctionProperty {
            r#type: "array".to_string(),
            description:
                "Async tool_call_id values to wait for. Omit to wait for all pending async tools."
                    .to_string(),
            extra: tool_call_ids_extra,
        },
    );

    llm::Tool {
        r#type: llm::ToolType::Function,
        function: Function {
            name: WAIT_TOOLS_NAME.to_string(),
            description: "Wait for pending async tool calls by tool_call_id. Use this after starting async tools when you need their results before answering; do not start the same tool again while its id is pending.".to_string(),
            parameters: Some(FunctionParameters {
                param_type: "object".to_string(),
                properties,
                required: None,
                extra: Map::new(),
            }),
        },
    }
}

fn check_tools_definition() -> llm::Tool {
    llm::Tool {
        r#type: llm::ToolType::Function,
        function: Function {
            name: CHECK_TOOLS_NAME.to_string(),
            description: "Check the status of pending async tool calls without blocking. Returns each outstanding call's status (running or finished) and a tail of its latest progress output. Use this to poll async work while you keep going instead of waiting.".to_string(),
            parameters: Some(FunctionParameters {
                param_type: "object".to_string(),
                properties: HashMap::new(),
                required: None,
                extra: Map::new(),
            }),
        },
    }
}

/// Appends a progress delta to a per-call rolling tail, truncating from the
/// front to stay within [`PROGRESS_TAIL_CAP`] bytes on a char boundary.
fn append_progress_tail(buffers: &mut HashMap<String, String>, id: &str, delta: &str) {
    let buffer = buffers.entry(id.to_string()).or_default();
    buffer.push_str(delta);
    if buffer.len() > PROGRESS_TAIL_CAP {
        let mut start = buffer.len() - PROGRESS_TAIL_CAP;
        while start < buffer.len() && !buffer.is_char_boundary(start) {
            start += 1;
        }
        *buffer = buffer[start..].to_string();
    }
}

/// Builds the non-blocking `check_tools` result: one entry per outstanding
/// async call plus any finished-but-not-yet-injected (deferred) completions.
fn check_tools_result(
    outstanding_async: &HashSet<String>,
    deferred: &[ToolEvent],
    progress_buffers: &HashMap<String, String>,
    async_names: &HashMap<String, String>,
) -> Value {
    let finished: HashMap<&str, &str> = deferred
        .iter()
        .filter_map(|event| match event {
            ToolEvent::Finished { id, name, .. } => Some((id.as_str(), name.as_str())),
            ToolEvent::Progress { .. } => None,
        })
        .collect();

    let mut tasks: Vec<Value> = Vec::new();
    for id in outstanding_async {
        let (status, tool_name) = match finished.get(id.as_str()) {
            Some(name) => ("finished", *name),
            None => (
                "running",
                async_names.get(id).map(String::as_str).unwrap_or(""),
            ),
        };
        tasks.push(json!({
            "tool_call_id": id,
            "tool_name": tool_name,
            "status": status,
            "progress_tail": progress_buffers.get(id).cloned().unwrap_or_default(),
        }));
    }
    for (id, name) in &finished {
        if !outstanding_async.contains(*id) {
            tasks.push(json!({
                "tool_call_id": id,
                "tool_name": name,
                "status": "finished",
                "progress_tail": progress_buffers.get(*id).cloned().unwrap_or_default(),
            }));
        }
    }

    json!({ "tasks": tasks })
}

fn async_placeholder(status: &str, tool_call_id: &str, name: &str) -> Value {
    json!({
        "status": status,
        "async": true,
        "tool_call_id": tool_call_id,
        "tool_name": name,
        "wait_with": {
            "tool": WAIT_TOOLS_NAME,
            "tool_call_ids": [tool_call_id],
        },
    })
}

fn wait_tool_ids(args: &Value, outstanding_async: &HashSet<String>) -> Vec<String> {
    let ids = args
        .get("tool_call_ids")
        .or_else(|| args.get("ids"))
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| outstanding_async.iter().cloned().collect());

    if ids.is_empty() {
        outstanding_async.iter().cloned().collect()
    } else {
        ids
    }
}

fn deferred_has_finished(deferred: &[ToolEvent], tool_call_id: &str) -> bool {
    deferred.iter().any(|event| {
        matches!(
            event,
            ToolEvent::Finished { id, .. } if id == tool_call_id
        )
    })
}

fn async_tool_key(name: &str, args: &Value) -> String {
    format!("{name}:{}", canonical_json(args))
}

fn canonical_json(value: &Value) -> String {
    match value {
        Value::Object(object) => {
            let mut entries: Vec<_> = object.iter().collect();
            entries.sort_by_key(|(key, _)| *key);
            let body = entries
                .into_iter()
                .map(|(key, value)| format!("{key}:{}", canonical_json(value)))
                .collect::<Vec<_>>()
                .join(",");
            format!("{{{body}}}")
        }
        Value::Array(values) => {
            let body = values
                .iter()
                .map(canonical_json)
                .collect::<Vec<_>>()
                .join(",");
            format!("[{body}]")
        }
        _ => serde_json::to_string(value).unwrap_or_else(|_| "null".to_string()),
    }
}

/// Strips the `__display` convention key from a tool result, returning the
/// human-facing rendering the tool supplied out of band, if any.
fn take_tool_display(result: &mut Value) -> Option<String> {
    result
        .as_object_mut()?
        .remove("__display")?
        .as_str()
        .map(str::to_string)
}

/// Injects a backgrounded tool's final result into the thread as a user message
/// so the model reads it on the next iteration. The assistant tool_call was
/// already satisfied by the placeholder, so a second Role::Tool message would be
/// invalid; a user follow-up keeps provider ordering valid. Returns the exact
/// injected message so the host can persist an identical one.
fn inject_async_result(
    agent: &Rc<RefCell<BaseAgentInner>>,
    id: &str,
    name: &str,
    result: &Value,
) -> String {
    let content =
        serde_json::to_string(result).unwrap_or_else(|err| format!(r#"{{"error":"{}"}}"#, err));
    let message = format!("Async task `{name}` (call {id}) finished:\n{content}");
    agent.borrow_mut().thread.push(Message {
        role: llm::Role::User,
        content: message.clone(),
        ..Default::default()
    });
    message
}

/// Spawns a backgrounded tool call on the host executor. The task streams
/// progress through the sink and delivers its final result over the shared
/// channel so the monitor loop can inject it into the live thread.
/// A tool call to run in the background.
struct AsyncToolTask {
    tool: Arc<dyn Tool>,
    config: Arc<AgentConfig>,
    args: Value,
    id: String,
    name: String,
    format: ToolOutputFormat,
}

fn spawn_tool_task(
    spawner: &Rc<dyn TaskSpawner>,
    task: AsyncToolTask,
    tx: mpsc::UnboundedSender<ToolEvent>,
) {
    let AsyncToolTask {
        tool,
        config,
        args,
        id,
        name,
        format,
    } = task;
    let sink = ToolProgressSink {
        id: id.clone(),
        name: name.clone(),
        format,
        tx: tx.clone(),
    };
    let spawn_id = id.clone();
    spawner.spawn(
        &spawn_id,
        Box::pin(async move {
            let mut result = tool.execute_streaming(config, args, sink).await;
            let display = take_tool_display(&mut result);
            let _ = tx.unbounded_send(ToolEvent::Finished {
                id,
                name,
                result,
                display,
                format,
            });
        }),
    );
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

fn append_tool_result(
    agent: &Rc<RefCell<BaseAgentInner>>,
    id: String,
    mut result: Value,
    readable_name: String,
    pending_images: &mut Vec<ImageSource>,
) -> String {
    pending_images.extend(take_tool_images(&mut result));

    let content =
        serde_json::to_string(&result).unwrap_or_else(|err| format!(r#"{{"error":"{}"}}"#, err));

    let mut lock = agent.borrow_mut();
    lock.thread.push(Message {
        role: llm::Role::Tool,
        content: content.clone(),
        tool_call_id: Some(id),
        ..Default::default()
    });
    let idx = lock.thread.len() - 1;
    lock.tool_display_names.insert(idx, readable_name);
    content
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use async_trait::async_trait;
    use futures::{FutureExt, StreamExt, pin_mut};
    use llm::{
        CompletionChoice, Delta, Function, FunctionParameters, StreamResponseChunk, ToolCallChunk,
        ToolCallFunction,
    };

    use super::*;

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
                }),
                String::new(),
                vec![],
            );

            let stream = agent
                .make_turn(
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
                }),
                "Use short answers.".to_string(),
                vec![Message {
                    role: llm::Role::User,
                    content: "previous".to_string(),
                    ..Default::default()
                }],
            );
            agent.register_tool(ApprovalTool { calls });

            let stream = agent.make_turn("next".to_string(), vec![]).await;
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

            let stream = agent.make_turn("run tool".to_string(), vec![]).await;
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
                | AgentResponseChunk::ToolProgress { .. }
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

            let stream = agent.make_turn("run tool".to_string(), vec![]).await;
            pin_mut!(stream);

            stream.next().await.unwrap().unwrap();
            stream.next().await.unwrap().unwrap();

            match stream.next().await.unwrap().unwrap() {
                AgentResponseChunk::Approval { approved, .. } => approved.send(false).unwrap(),
                AgentResponseChunk::Chunk(_)
                | AgentResponseChunk::Quiz { .. }
                | AgentResponseChunk::ToolStarted { .. }
                | AgentResponseChunk::ToolProgress { .. }
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

    fn tool_call_chunk(arguments: &str) -> StreamResponseChunk {
        StreamResponseChunk {
            id: "chunk".to_string(),
            object: "mock.stream".to_string(),
            created: 0,
            model: "mock-model".to_string(),
            system_fingerprint: None,
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
        named_tool_call_chunk_with_id("call_1", name, arguments)
    }

    fn named_tool_call_chunk_with_id(id: &str, name: &str, arguments: &str) -> StreamResponseChunk {
        StreamResponseChunk {
            id: "chunk".to_string(),
            object: "mock.stream".to_string(),
            created: 0,
            model: "mock-model".to_string(),
            system_fingerprint: None,
            choices: vec![CompletionChoice {
                message: None,
                text: None,
                index: 0,
                delta: Some(Delta {
                    content: None,
                    thinking: None,
                    tool_calls: Some(vec![ToolCallChunk {
                        index: Some(0),
                        id: Some(id.to_string()),
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
        group: Option<&'static str>,
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

        fn searchable_group(&self) -> Option<String> {
            self.group.map(str::to_string)
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
            }),
            String::new(),
            vec![],
        );
        agent.set_searchable_tools_preview_limit(2);
        agent.register_searchable_tool(NamedSearchableTool {
            name: "alpha",
            group: None,
        });
        agent.register_searchable_tool(NamedSearchableTool {
            name: "beta",
            group: None,
        });
        agent.register_searchable_tool(NamedSearchableTool {
            name: "gamma",
            group: None,
        });
        agent.register_searchable_tool(NamedSearchableTool {
            name: "github_create_issue",
            group: Some("GitHub MCP"),
        });
        agent.register_searchable_tool(NamedSearchableTool {
            name: "github_list_issues",
            group: Some("GitHub MCP"),
        });

        let description = agent
            .tool_definitions()
            .into_iter()
            .find(|tool| tool.function.name == "search_tools")
            .unwrap()
            .function
            .description;

        assert!(description.contains("5 additional tools"));
        assert!(description.contains("Tool names include: alpha, beta and 1 more."));
        assert!(description.contains("MCP servers include: GitHub MCP (2 tools)."));
        assert!(!description.contains("gamma,"));
        assert!(!description.contains("github_create_issue"));
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
                }),
                String::new(),
                vec![],
            );
            agent.register_searchable_tool(EchoTool);

            let stream = agent.make_turn("go".to_string(), vec![]).await;
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

    /// Drives each spawned future to completion synchronously. Enough to
    /// exercise the monitor path in tests, where tool futures resolve without
    /// real I/O.
    struct ImmediateSpawner;

    impl TaskSpawner for ImmediateSpawner {
        fn spawn(
            &self,
            _id: &str,
            mut future: std::pin::Pin<Box<dyn std::future::Future<Output = ()>>>,
        ) {
            let waker = std::task::Waker::noop();
            let mut cx = std::task::Context::from_waker(waker);
            while future.as_mut().poll(&mut cx).is_pending() {}
        }
    }

    type SpawnedTask = Pin<Box<dyn std::future::Future<Output = ()>>>;

    #[derive(Clone, Default)]
    struct ManualSpawner {
        tasks: Rc<RefCell<Vec<SpawnedTask>>>,
    }

    impl ManualSpawner {
        fn task_count(&self) -> usize {
            self.tasks.borrow().len()
        }

        fn run_one(&self) {
            let mut future = self.tasks.borrow_mut().pop().unwrap();
            let waker = std::task::Waker::noop();
            let mut cx = std::task::Context::from_waker(waker);
            while future.as_mut().poll(&mut cx).is_pending() {}
        }
    }

    impl TaskSpawner for ManualSpawner {
        fn spawn(&self, _id: &str, future: Pin<Box<dyn std::future::Future<Output = ()>>>) {
            self.tasks.borrow_mut().push(future);
        }
    }

    struct StreamingTool;

    #[async_trait(?Send)]
    impl Tool for StreamingTool {
        fn name(&self) -> &str {
            "streamer"
        }
        fn readable_name(&self) -> &str {
            "Streamer"
        }
        fn definition(&self) -> llm::Tool {
            llm::Tool {
                r#type: llm::ToolType::Function,
                function: Function {
                    description: "streams".to_string(),
                    name: self.name().to_string(),
                    parameters: Some(FunctionParameters {
                        param_type: "object".to_string(),
                        ..Default::default()
                    }),
                },
            }
        }
        fn streams(&self) -> bool {
            true
        }
        fn output_format(&self) -> ToolOutputFormat {
            ToolOutputFormat::Markdown
        }
        async fn execute(&self, _config: Arc<AgentConfig>, _args: Value) -> Value {
            json!({ "done": true })
        }
        async fn execute_streaming(
            &self,
            _config: Arc<AgentConfig>,
            _args: Value,
            sink: crate::ToolProgressSink,
        ) -> Value {
            sink.progress("chunk-a");
            sink.progress("chunk-b");
            json!({ "done": true })
        }
    }

    struct AsyncTool {
        calls: Arc<AtomicUsize>,
        last_args: Arc<Mutex<Option<Value>>>,
    }

    #[async_trait(?Send)]
    impl Tool for AsyncTool {
        fn name(&self) -> &str {
            "background"
        }
        fn readable_name(&self) -> &str {
            "Background"
        }
        fn definition(&self) -> llm::Tool {
            llm::Tool {
                r#type: llm::ToolType::Function,
                function: Function {
                    description: "runs in background".to_string(),
                    name: self.name().to_string(),
                    parameters: Some(FunctionParameters {
                        param_type: "object".to_string(),
                        ..Default::default()
                    }),
                },
            }
        }
        fn supports_async(&self) -> bool {
            true
        }
        fn output_format(&self) -> ToolOutputFormat {
            ToolOutputFormat::Markdown
        }
        async fn execute(&self, _config: Arc<AgentConfig>, args: Value) -> Value {
            self.calls.fetch_add(1, Ordering::SeqCst);
            *self.last_args.lock().unwrap() = Some(args);
            json!({ "ok": true, "__display": "async output" })
        }
    }

    #[test]
    fn streaming_tool_emits_progress() {
        futures::executor::block_on(async {
            let mock = llm::Mock::new().with_stream_chunks(vec![
                vec![named_tool_call_chunk("streamer", "{}")],
                vec![text_chunk("done")],
            ]);
            let agent = BaseAgent::new(
                DEFAULT_MODEL.to_string(),
                Arc::new(AgentConfig {
                    model_registry: registry(&mock),
                    max_iterations: 50,
                }),
                String::new(),
                vec![],
            );
            agent.register_tool(StreamingTool);

            let stream = agent.make_turn("go".to_string(), vec![]).await;
            pin_mut!(stream);
            let mut deltas = Vec::new();
            let mut markdown_progress = false;
            while let Some(chunk) = stream.next().await {
                if let Ok(AgentResponseChunk::ToolProgress { delta, format, .. }) = &chunk {
                    deltas.push(delta.clone());
                    markdown_progress = *format == ToolOutputFormat::Markdown;
                }
            }
            assert_eq!(deltas, vec!["chunk-a".to_string(), "chunk-b".to_string()]);
            assert!(markdown_progress);
        });
    }

    #[test]
    fn async_tool_runs_in_background_and_wakes() {
        futures::executor::block_on(async {
            let mock = llm::Mock::new().with_stream_chunks(vec![
                vec![named_tool_call_chunk("background", r#"{"async":true}"#)],
                vec![text_chunk("done")],
            ]);
            let agent = BaseAgent::new(
                DEFAULT_MODEL.to_string(),
                Arc::new(AgentConfig {
                    model_registry: registry(&mock),
                    max_iterations: 50,
                }),
                String::new(),
                vec![],
            );
            let calls = Arc::new(AtomicUsize::new(0));
            let last_args = Arc::new(Mutex::new(None));
            agent.register_tool(AsyncTool {
                calls: calls.clone(),
                last_args: last_args.clone(),
            });
            agent.set_spawner(Rc::new(ImmediateSpawner));

            let stream = agent.make_turn("go".to_string(), vec![]).await;
            pin_mut!(stream);
            let mut final_display = None;
            let mut final_format = None;
            while let Some(chunk) = stream.next().await {
                if let Ok(AgentResponseChunk::ToolFinished {
                    display: Some(display),
                    format,
                    ..
                }) = &chunk
                {
                    final_display = Some(display.clone());
                    final_format = Some(*format);
                }
            }

            assert_eq!(calls.load(Ordering::SeqCst), 1);
            assert_eq!(final_display.as_deref(), Some("async output"));
            assert_eq!(final_format, Some(ToolOutputFormat::Markdown));
            // The tool never saw the reserved `async` flag.
            assert!(
                last_args
                    .lock()
                    .unwrap()
                    .as_ref()
                    .and_then(|args| args.get("async"))
                    .is_none()
            );
            // The finished result was injected as a user follow-up so the model
            // could react to it.
            assert!(
                agent
                    .thread()
                    .iter()
                    .any(|message| message.role == llm::Role::User
                        && message.content.contains("Async task"))
            );
        });
    }

    #[test]
    fn async_tool_exposes_wait_tools_join() {
        futures::executor::block_on(async {
            let mock = llm::Mock::new().with_stream_chunks(vec![
                vec![named_tool_call_chunk_with_id(
                    "async_1",
                    "background",
                    r#"{"async":true}"#,
                )],
                vec![named_tool_call_chunk_with_id(
                    "wait_1",
                    WAIT_TOOLS_NAME,
                    r#"{"tool_call_ids":["async_1"]}"#,
                )],
                vec![text_chunk("done")],
            ]);
            let agent = BaseAgent::new(
                DEFAULT_MODEL.to_string(),
                Arc::new(AgentConfig {
                    model_registry: registry(&mock),
                    max_iterations: 50,
                }),
                String::new(),
                vec![],
            );
            agent.register_tool(AsyncTool {
                calls: Arc::new(AtomicUsize::new(0)),
                last_args: Arc::new(Mutex::new(None)),
            });
            let spawner = ManualSpawner::default();
            agent.set_spawner(Rc::new(spawner.clone()));

            let stream = agent.make_turn("go".to_string(), vec![]).await;
            pin_mut!(stream);

            assert!(matches!(
                stream.next().await.unwrap().unwrap(),
                AgentResponseChunk::Chunk(_)
            ));
            assert!(matches!(
                stream.next().await.unwrap().unwrap(),
                AgentResponseChunk::ToolStarted { tool_call_id, .. } if tool_call_id == "async_1"
            ));
            assert!(matches!(
                stream.next().await.unwrap().unwrap(),
                AgentResponseChunk::ToolFinished {
                    tool_call_id,
                    wake_message: None,
                    ..
                } if tool_call_id == "async_1"
            ));
            assert_eq!(spawner.task_count(), 1);

            assert!(matches!(
                stream.next().await.unwrap().unwrap(),
                AgentResponseChunk::Chunk(_)
            ));
            assert!(
                mock.stream_requests()[1]
                    .tools
                    .as_ref()
                    .unwrap()
                    .iter()
                    .any(|tool| tool.function.name == WAIT_TOOLS_NAME)
            );
            assert!(matches!(
                stream.next().await.unwrap().unwrap(),
                AgentResponseChunk::ToolStarted { tool_call_id, name, .. }
                    if tool_call_id == "wait_1" && name == WAIT_TOOLS_NAME
            ));
            assert!(stream.next().now_or_never().is_none());

            spawner.run_one();

            assert!(matches!(
                stream.next().await.unwrap().unwrap(),
                AgentResponseChunk::ToolFinished {
                    tool_call_id,
                    name,
                    wake_message: None,
                    ..
                } if tool_call_id == "wait_1" && name == WAIT_TOOLS_NAME
            ));
            assert!(matches!(
                stream.next().await.unwrap().unwrap(),
                AgentResponseChunk::ToolFinished {
                    tool_call_id,
                    wake_message: Some(_),
                    ..
                } if tool_call_id == "async_1"
            ));

            let mut saw_done = false;
            while let Some(chunk) = stream.next().await {
                if chunk_text(&chunk.unwrap()) == Some("done") {
                    saw_done = true;
                }
            }

            assert!(saw_done);
        });
    }

    #[test]
    fn repeated_pending_async_tool_call_reuses_existing_handle() {
        futures::executor::block_on(async {
            let calls = Arc::new(AtomicUsize::new(0));
            let mock = llm::Mock::new().with_stream_chunks(vec![
                vec![named_tool_call_chunk_with_id(
                    "async_1",
                    "background",
                    r#"{"async":true,"topic":"same"}"#,
                )],
                vec![named_tool_call_chunk_with_id(
                    "retry_1",
                    "background",
                    r#"{"topic":"same"}"#,
                )],
            ]);
            let agent = BaseAgent::new(
                DEFAULT_MODEL.to_string(),
                Arc::new(AgentConfig {
                    model_registry: registry(&mock),
                    max_iterations: 50,
                }),
                String::new(),
                vec![],
            );
            agent.register_tool(AsyncTool {
                calls: calls.clone(),
                last_args: Arc::new(Mutex::new(None)),
            });
            let spawner = ManualSpawner::default();
            agent.set_spawner(Rc::new(spawner.clone()));

            let stream = agent.make_turn("go".to_string(), vec![]).await;
            pin_mut!(stream);

            for _ in 0..3 {
                stream.next().await.unwrap().unwrap();
            }
            assert_eq!(spawner.task_count(), 1);

            assert!(matches!(
                stream.next().await.unwrap().unwrap(),
                AgentResponseChunk::Chunk(_)
            ));
            assert!(matches!(
                stream.next().await.unwrap().unwrap(),
                AgentResponseChunk::ToolStarted { tool_call_id, .. } if tool_call_id == "retry_1"
            ));
            match stream.next().await.unwrap().unwrap() {
                AgentResponseChunk::ToolFinished {
                    tool_call_id,
                    result,
                    wake_message,
                    ..
                } => {
                    assert_eq!(tool_call_id, "retry_1");
                    assert!(result.contains("already_started"));
                    assert!(result.contains("async_1"));
                    assert!(wake_message.is_none());
                }
                _ => panic!("expected duplicate placeholder"),
            }

            assert_eq!(spawner.task_count(), 1);
            assert_eq!(calls.load(Ordering::SeqCst), 0);
        });
    }

    #[test]
    fn tool_started_carries_background_flag() {
        futures::executor::block_on(async {
            let mock = llm::Mock::new().with_stream_chunks(vec![
                vec![named_tool_call_chunk_with_id(
                    "async_1",
                    "background",
                    r#"{"async":true}"#,
                )],
                vec![text_chunk("done")],
            ]);
            let agent = BaseAgent::new(
                DEFAULT_MODEL.to_string(),
                Arc::new(AgentConfig {
                    model_registry: registry(&mock),
                    max_iterations: 50,
                }),
                String::new(),
                vec![],
            );
            agent.register_tool(AsyncTool {
                calls: Arc::new(AtomicUsize::new(0)),
                last_args: Arc::new(Mutex::new(None)),
            });
            let spawner = ManualSpawner::default();
            agent.set_spawner(Rc::new(spawner.clone()));

            let stream = agent.make_turn("go".to_string(), vec![]).await;
            pin_mut!(stream);

            let mut background_flag = None;
            while let Some(chunk) = stream.next().await {
                if let Ok(AgentResponseChunk::ToolStarted { background, .. }) = &chunk {
                    background_flag = Some(*background);
                    break;
                }
            }
            assert_eq!(background_flag, Some(true));
        });
    }

    #[test]
    fn tool_started_background_false_for_sync_tool() {
        futures::executor::block_on(async {
            let calls = Arc::new(AtomicUsize::new(0));
            let mock = llm::Mock::new().with_stream_chunks(vec![
                vec![tool_call_chunk(r#"{"value":1}"#)],
                vec![text_chunk("done")],
            ]);
            let agent = make_agent(&mock, calls);

            let stream = agent.make_turn("go".to_string(), vec![]).await;
            pin_mut!(stream);

            let mut background_flag = None;
            while let Some(chunk) = stream.next().await {
                match chunk.unwrap() {
                    AgentResponseChunk::ToolStarted { background, .. } => {
                        background_flag = Some(background);
                        break;
                    }
                    AgentResponseChunk::Approval { approved, .. } => {
                        let _ = approved.send(false);
                    }
                    _ => {}
                }
            }
            assert_eq!(background_flag, Some(false));
        });
    }

    #[test]
    fn check_tools_reports_running_then_finished() {
        futures::executor::block_on(async {
            let mock = llm::Mock::new().with_stream_chunks(vec![
                vec![named_tool_call_chunk_with_id(
                    "async_1",
                    "background",
                    r#"{"async":true}"#,
                )],
                vec![named_tool_call_chunk_with_id(
                    "check_1",
                    CHECK_TOOLS_NAME,
                    "{}",
                )],
                vec![named_tool_call_chunk_with_id(
                    "check_2",
                    CHECK_TOOLS_NAME,
                    "{}",
                )],
                vec![text_chunk("done")],
            ]);
            let agent = BaseAgent::new(
                DEFAULT_MODEL.to_string(),
                Arc::new(AgentConfig {
                    model_registry: registry(&mock),
                    max_iterations: 50,
                }),
                String::new(),
                vec![],
            );
            agent.register_tool(AsyncTool {
                calls: Arc::new(AtomicUsize::new(0)),
                last_args: Arc::new(Mutex::new(None)),
            });
            let spawner = ManualSpawner::default();
            agent.set_spawner(Rc::new(spawner.clone()));

            let stream = agent.make_turn("go".to_string(), vec![]).await;
            pin_mut!(stream);

            let running = check_tools_content(stream.as_mut(), "check_1").await;
            let running: Value = serde_json::from_str(&running).unwrap();
            let tasks = running["tasks"].as_array().unwrap();
            assert_eq!(tasks.len(), 1);
            assert_eq!(tasks[0]["tool_call_id"], "async_1");
            assert_eq!(tasks[0]["status"], "running");
            assert_eq!(tasks[0]["tool_name"], "Background");

            spawner.run_one();

            let finished = check_tools_content(stream.as_mut(), "check_2").await;
            let finished: Value = serde_json::from_str(&finished).unwrap();
            let tasks = finished["tasks"].as_array().unwrap();
            assert_eq!(tasks.len(), 1);
            assert_eq!(tasks[0]["tool_call_id"], "async_1");
            assert_eq!(tasks[0]["status"], "finished");

            while stream.next().await.is_some() {}
        });
    }

    async fn check_tools_content(
        mut stream: Pin<&mut impl Stream<Item = Result<AgentResponseChunk, AgentError>>>,
        target_id: &str,
    ) -> String {
        while let Some(chunk) = stream.next().await {
            if let Ok(AgentResponseChunk::ToolFinished {
                tool_call_id,
                name,
                result,
                ..
            }) = chunk
                && name == CHECK_TOOLS_NAME
                && tool_call_id == target_id
            {
                return result;
            }
        }
        panic!("check_tools result for {target_id} not seen");
    }

    #[test]
    fn async_without_spawner_runs_inline() {
        futures::executor::block_on(async {
            let mock = llm::Mock::new().with_stream_chunks(vec![
                vec![named_tool_call_chunk("background", r#"{"async":true}"#)],
                vec![text_chunk("done")],
            ]);
            let agent = BaseAgent::new(
                DEFAULT_MODEL.to_string(),
                Arc::new(AgentConfig {
                    model_registry: registry(&mock),
                    max_iterations: 50,
                }),
                String::new(),
                vec![],
            );
            let calls = Arc::new(AtomicUsize::new(0));
            agent.register_tool(AsyncTool {
                calls: calls.clone(),
                last_args: Arc::new(Mutex::new(None)),
            });
            // No spawner installed.

            let stream = agent.make_turn("go".to_string(), vec![]).await;
            pin_mut!(stream);
            while stream.next().await.is_some() {}

            assert_eq!(calls.load(Ordering::SeqCst), 1);
            // Ran inline, so no async follow-up user message was injected.
            assert!(
                !agent
                    .thread()
                    .iter()
                    .any(|message| message.content.contains("Async task"))
            );
        });
    }
}
