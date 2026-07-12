use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::Arc};

use futures::StreamExt;
use minisql::{ConnectionPool, Value};
use stride_agent::{
    EventKind, EventSink, MessageRole, ThreadEvent, TurnContext,
    sanitizer::{HtmlFormattingSanitizer, StreamingMessageSanitizer},
};
use tokio::sync::{mpsc, watch};
use uuid::Uuid;

use crate::{
    db::{MessageFormat, Role, messages, thread_agents, thread_events, threads},
    model_registry,
    runner::{
        AgentEvent, AgentEventKind, AgentPoolError, PartialAgentMessage, RunId, db_error,
        pool::{WorkerState, drain_queue, with_runner},
        thread_events_topic,
    },
};

/// Number of most-recent runs whose structural events are retained in the
/// journal. Older runs are pruned when a run completes; the journal exists for
/// reconnect replay and event attachment, not audit, so it stays small.
const JOURNAL_RETAINED_RUNS: usize = 5;

struct AssistantMessageState {
    content: String,
    thinking: Option<String>,
    format: MessageFormat,
    output_sanitizer: Box<dyn StreamingMessageSanitizer>,
}

#[derive(Default)]
struct TurnPersistence {
    assistants: HashMap<Uuid, AssistantMessageState>,
    last_root_message_id: Option<Uuid>,
    tool_calls: Vec<PartialToolCall>,
    tool_results: HashMap<String, (String, String)>,
    next_tool_result: usize,
    tool_calls_message_id: Option<Uuid>,
    /// Per-subagent persistence, keyed by the subagent's encoded `agent_path`.
    subagents: HashMap<String, SubagentPersistence>,
}

/// Accumulates a subagent's assistant text in memory and writes each message
/// once at commit (D-3c: no per-delta DB writes for subagents). Tool calls and
/// results persist into the same message columns the root agent uses.
#[derive(Default)]
struct SubagentPersistence {
    content: String,
    thinking: Option<String>,
    current_message_id: Option<Uuid>,
    last_message_id: Option<Uuid>,
    tool_calls: Vec<PartialToolCall>,
    tool_results: HashMap<String, (String, String)>,
    next_tool_result: usize,
    tool_calls_message_id: Option<Uuid>,
}

#[derive(Default)]
struct PartialToolCall {
    id: String,
    name: String,
    arguments: String,
}

#[derive(Default)]
struct RawMarkdownSanitizer {
    output: String,
}

#[derive(Clone)]
struct ServerEventSink {
    sender: mpsc::UnboundedSender<ThreadEvent>,
}

impl EventSink for ServerEventSink {
    fn emit(&self, event: ThreadEvent) {
        let _ = self.sender.send(event);
    }
}

impl StreamingMessageSanitizer for RawMarkdownSanitizer {
    fn push_str(&mut self, chunk: &str) {
        self.output.push_str(chunk);
    }

    fn snapshot(&self) -> String {
        self.output.clone()
    }

    fn finish(&mut self) -> String {
        self.output.clone()
    }
}

pub(crate) async fn run_agent_turn(
    state: Rc<RefCell<WorkerState>>,
    thread_id: Uuid,
    run_id: RunId,
    content: String,
    images: Vec<llm::ImageSource>,
    model: Option<String>,
    mut cancel_rx: watch::Receiver<bool>,
) {
    let agent = {
        let mut state = state.borrow_mut();
        let Some(runner) = state.threads.get_mut(&thread_id) else {
            return;
        };
        runner.agent.take()
    };

    let Some(agent) = agent else {
        fail_run(
            &state,
            thread_id,
            run_id,
            "agent is already running".to_string(),
        )
        .await;
        return;
    };

    let resolved_model =
        match model_registry::resolve_chat_model(&agent.model_registry(), model.as_deref()) {
            Ok(key) => key,
            Err(error) => {
                fail_run(&state, thread_id, run_id, error).await;
                restore_agent(&state, thread_id, agent);
                drain_queue(&state, thread_id).await;
                return;
            }
        };
    persist_thread_model(&state, thread_id, &resolved_model).await;
    agent.set_model(resolved_model);

    let format = thread_message_format(&state, thread_id).unwrap_or(MessageFormat::Markdown);
    let broker = {
        let state = state.borrow();
        let Some(runner) = state.threads.get(&thread_id) else {
            return;
        };
        runner.broker.clone()
    };
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    let context = TurnContext::new(
        run_id.0,
        Arc::new(ServerEventSink { sender: event_tx }),
        broker,
    )
    .without_user_message_events();
    let mut stream = agent
        .make_turn(with_format_reminder(content, format), images, context)
        .await;
    let mut persistence = TurnPersistence::default();
    let mut source_finished = false;

    while !source_finished {
        tokio::select! {
            biased;
            _ = cancel_rx.changed() => {
                cancel_run_task(&state, thread_id, run_id).await;
                restore_agent(&state, thread_id, agent);
                drain_queue(&state, thread_id).await;
                return;
            }
            event = event_rx.recv() => {
                if let Some(event) = event
                    && let Err(error) = process_thread_event(
                        &state,
                        thread_id,
                        run_id,
                        format,
                        &mut persistence,
                        event,
                    ).await
                {
                    fail_run(&state, thread_id, run_id, error.to_string()).await;
                    restore_agent(&state, thread_id, agent);
                    drain_queue(&state, thread_id).await;
                    return;
                }
            }
            item = stream.next() => source_finished = item.is_none(),
        }
    }

    while let Ok(event) = event_rx.try_recv() {
        if let Err(error) =
            process_thread_event(&state, thread_id, run_id, format, &mut persistence, event).await
        {
            fail_run(&state, thread_id, run_id, error.to_string()).await;
            restore_agent(&state, thread_id, agent);
            drain_queue(&state, thread_id).await;
            return;
        }
    }

    let clock = state.borrow().init.config.clock.clone();
    with_runner(&state, thread_id, |runner| {
        runner.cancel_tx = None;
        runner.broker.clear();
        runner.status = crate::runner::ThreadStatus::Idle;
        runner.in_progress = None;
        runner.last_used = clock.now_instant();
    });
    restore_agent(&state, thread_id, agent);
    drain_queue(&state, thread_id).await;
}

async fn persist_thread_model(state: &Rc<RefCell<WorkerState>>, thread_id: Uuid, model: &str) {
    let db = state.borrow().init.db.clone();
    if let Err(error) = threads::update()
        .last_model(Some(model))
        .where_(threads::id.eq(thread_id))
        .execute(&db)
        .await
    {
        tracing::warn!(%thread_id, %model, %error, "failed to persist thread model");
    }
}

async fn process_thread_event(
    state: &Rc<RefCell<WorkerState>>,
    thread_id: Uuid,
    run_id: RunId,
    format: MessageFormat,
    persistence: &mut TurnPersistence,
    event: ThreadEvent,
) -> Result<(), AgentPoolError> {
    let is_root = event.agent_path.is_empty();
    if !is_root {
        persist_subagent_event(state, thread_id, persistence, &event).await?;
    }
    match &event.kind {
        EventKind::MessageStarted {
            message_id,
            role: MessageRole::Assistant,
        } if is_root => {
            let seq = crate::runner::pool::next_message_seq(state, thread_id)?;
            let db = state.borrow().init.db.clone();
            messages::insert()
                .id(*message_id)
                .parent_thread(thread_id)
                .seq(seq)
                .role(Role::Agent)
                .content("")
                .content_format(format)
                .images(Option::<&str>::None)
                .thinking(Option::<&str>::None)
                .tool_calls(Option::<&str>::None)
                .tool_call_id(Option::<&str>::None)
                .agent_path(Option::<&str>::None)
                .execute(&db)
                .await
                .map_err(db_error)?;
            persistence.assistants.insert(
                *message_id,
                AssistantMessageState {
                    content: String::new(),
                    thinking: None,
                    format,
                    output_sanitizer: output_sanitizer(format, state),
                },
            );
        }
        EventKind::TextDelta { message_id, delta } if is_root => {
            if let Some(assistant) = persistence.assistants.get_mut(message_id) {
                assistant.output_sanitizer.push_str(delta);
                assistant.content = assistant.output_sanitizer.snapshot();
                let content = assistant.content.clone();
                let thinking = assistant.thinking.clone();
                let db = state.borrow().init.db.clone();
                update_message(&db, *message_id, &content, thinking.as_deref(), None).await?;
                with_runner(state, thread_id, |runner| {
                    runner.in_progress = Some(PartialAgentMessage {
                        message_id: *message_id,
                        run_id,
                        content,
                        thinking,
                        format: assistant.format,
                    });
                });
            }
        }
        EventKind::ThinkingDelta { message_id, delta } if is_root => {
            if let Some(assistant) = persistence.assistants.get_mut(message_id) {
                assistant
                    .thinking
                    .get_or_insert_with(String::new)
                    .push_str(delta);
                let content = assistant.content.clone();
                let thinking = assistant.thinking.clone();
                let db = state.borrow().init.db.clone();
                update_message(&db, *message_id, &content, thinking.as_deref(), None).await?;
            }
        }
        EventKind::MessageCommitted { message_id } if is_root => {
            if let Some(mut assistant) = persistence.assistants.remove(message_id) {
                assistant.content = assistant.output_sanitizer.finish();
                let db = state.borrow().init.db.clone();
                update_message(
                    &db,
                    *message_id,
                    &assistant.content,
                    assistant.thinking.as_deref(),
                    None,
                )
                .await?;
                persistence.last_root_message_id = Some(*message_id);
                with_runner(state, thread_id, |runner| runner.in_progress = None);
            }
        }
        EventKind::ToolCallStarted {
            tool_call_id,
            name,
            arguments,
        } if is_root => {
            if persistence.tool_calls_message_id != persistence.last_root_message_id {
                persistence.tool_calls.clear();
                persistence.tool_results.clear();
                persistence.next_tool_result = 0;
                persistence.tool_calls_message_id = persistence.last_root_message_id;
            }
            persistence.tool_calls.push(PartialToolCall {
                id: tool_call_id.clone(),
                name: name.clone(),
                arguments: arguments.clone(),
            });
            if let Some(message_id) = persistence.last_root_message_id {
                let tool_calls = serialize_tool_calls(&persistence.tool_calls)?;
                let db = state.borrow().init.db.clone();
                messages::update()
                    .tool_calls(tool_calls.as_deref())
                    .where_(messages::id.eq(message_id))
                    .execute(&db)
                    .await
                    .map_err(db_error)?;
            }
        }
        EventKind::ToolCallFinished {
            tool_call_id,
            name,
            result,
            ..
        } if is_root => {
            persistence
                .tool_results
                .insert(tool_call_id.clone(), (name.clone(), result.clone()));
            while let Some(call) = persistence.tool_calls.get(persistence.next_tool_result) {
                let Some((_, result)) = persistence.tool_results.remove(&call.id) else {
                    break;
                };
                persist_tool_message(state, thread_id, &call.id, &result, None).await?;
                persistence.next_tool_result += 1;
            }
        }
        EventKind::RunFinished | EventKind::RunFailed { .. } if is_root => {
            let clock = state.borrow().init.config.clock.clone();
            with_runner(state, thread_id, |runner| {
                runner.cancel_tx = None;
                runner.broker.clear();
                runner.status = crate::runner::ThreadStatus::Idle;
                runner.in_progress = None;
                runner.last_used = clock.now_instant();
            });
        }
        EventKind::AgentSpawned {
            agent_id,
            parent_tool_call_id,
            name,
            model,
        } => {
            // The spawn event carries the *parent's* path; the child's own path
            // appends its id.
            let mut agent_path = event.agent_path.clone();
            agent_path.push(*agent_id);
            upsert_agent_spawned(
                state,
                thread_id,
                *agent_id,
                &agent_path,
                parent_tool_call_id,
                name,
                model,
            )
            .await?;
        }
        EventKind::AgentFinished { agent_id, result } => {
            mark_agent_finished(state, thread_id, *agent_id, result).await?;
        }
        _ => {}
    }

    emit_thread_event(state, thread_id, event).await;
    Ok(())
}

/// Persists a subagent's message/tool events. Assistant text accumulates in
/// memory and the row is written once at `MessageCommitted`; tool calls/results
/// land in the same columns the root agent uses, tagged with the subagent's
/// encoded `agent_path`.
async fn persist_subagent_event(
    state: &Rc<RefCell<WorkerState>>,
    thread_id: Uuid,
    persistence: &mut TurnPersistence,
    event: &ThreadEvent,
) -> Result<(), AgentPoolError> {
    let Some(key) = encode_agent_path(&event.agent_path) else {
        return Ok(());
    };
    match &event.kind {
        EventKind::MessageStarted {
            message_id,
            role: MessageRole::Assistant,
        } => {
            let sub = persistence.subagents.entry(key).or_default();
            sub.content.clear();
            sub.thinking = None;
            sub.current_message_id = Some(*message_id);
        }
        EventKind::TextDelta { message_id, delta } => {
            let sub = persistence.subagents.entry(key).or_default();
            if sub.current_message_id == Some(*message_id) {
                sub.content.push_str(delta);
            }
        }
        EventKind::ThinkingDelta { message_id, delta } => {
            let sub = persistence.subagents.entry(key).or_default();
            if sub.current_message_id == Some(*message_id) {
                sub.thinking.get_or_insert_with(String::new).push_str(delta);
            }
        }
        EventKind::MessageCommitted { message_id } => {
            let Some((content, thinking)) = ({
                let sub = persistence.subagents.entry(key.clone()).or_default();
                if sub.current_message_id != Some(*message_id) {
                    None
                } else {
                    sub.current_message_id = None;
                    sub.last_message_id = Some(*message_id);
                    Some((sub.content.clone(), sub.thinking.clone()))
                }
            }) else {
                return Ok(());
            };
            let seq = crate::runner::pool::next_message_seq(state, thread_id)?;
            let db = state.borrow().init.db.clone();
            messages::insert()
                .id(*message_id)
                .parent_thread(thread_id)
                .seq(seq)
                .role(Role::Agent)
                .content(content.as_str())
                .content_format(MessageFormat::Markdown)
                .images(Option::<&str>::None)
                .thinking(thinking.as_deref())
                .tool_calls(Option::<&str>::None)
                .tool_call_id(Option::<&str>::None)
                .agent_path(Some(key.as_str()))
                .execute(&db)
                .await
                .map_err(db_error)?;
        }
        EventKind::ToolCallStarted {
            tool_call_id,
            name,
            arguments,
        } => {
            let (message_id, tool_calls_json) = {
                let sub = persistence.subagents.entry(key.clone()).or_default();
                if sub.tool_calls_message_id != sub.last_message_id {
                    sub.tool_calls.clear();
                    sub.tool_results.clear();
                    sub.next_tool_result = 0;
                    sub.tool_calls_message_id = sub.last_message_id;
                }
                sub.tool_calls.push(PartialToolCall {
                    id: tool_call_id.clone(),
                    name: name.clone(),
                    arguments: arguments.clone(),
                });
                (sub.last_message_id, serialize_tool_calls(&sub.tool_calls)?)
            };
            if let Some(message_id) = message_id {
                let db = state.borrow().init.db.clone();
                messages::update()
                    .tool_calls(tool_calls_json.as_deref())
                    .where_(messages::id.eq(message_id))
                    .execute(&db)
                    .await
                    .map_err(db_error)?;
            }
        }
        EventKind::ToolCallFinished {
            tool_call_id,
            name,
            result,
            ..
        } => {
            {
                let sub = persistence.subagents.entry(key.clone()).or_default();
                sub.tool_results
                    .insert(tool_call_id.clone(), (name.clone(), result.clone()));
            }
            loop {
                let ready = {
                    let sub = persistence.subagents.entry(key.clone()).or_default();
                    match sub.tool_calls.get(sub.next_tool_result) {
                        Some(call) => sub
                            .tool_results
                            .remove(&call.id)
                            .map(|(_, result)| (call.id.clone(), result)),
                        None => None,
                    }
                };
                let Some((call_id, result)) = ready else {
                    break;
                };
                persist_tool_message(state, thread_id, &call_id, &result, Some(key.as_str()))
                    .await?;
                persistence
                    .subagents
                    .entry(key.clone())
                    .or_default()
                    .next_tool_result += 1;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Inserts a `thread_agents` row when a subagent is spawned.
async fn upsert_agent_spawned(
    state: &Rc<RefCell<WorkerState>>,
    thread_id: Uuid,
    agent_id: Uuid,
    agent_path: &[Uuid],
    parent_tool_call_id: &str,
    name: &str,
    model: &str,
) -> Result<(), AgentPoolError> {
    let (db, created_at) = {
        let state = state.borrow();
        (
            state.init.db.clone(),
            state.init.config.clock.now_unix_millis(),
        )
    };
    let path = encode_agent_path(agent_path).unwrap_or_default();
    thread_agents::insert()
        .agent_id(agent_id)
        .thread_id(thread_id)
        .agent_path(path.as_str())
        .parent_tool_call_id(Some(parent_tool_call_id))
        .name(name)
        .model(model)
        .result(Option::<&str>::None)
        .finished(false)
        .created_at(created_at)
        .execute(&db)
        .await
        .map_err(db_error)?;
    Ok(())
}

/// Marks a subagent finished and records its final result.
async fn mark_agent_finished(
    state: &Rc<RefCell<WorkerState>>,
    thread_id: Uuid,
    agent_id: Uuid,
    result: &str,
) -> Result<(), AgentPoolError> {
    let db = state.borrow().init.db.clone();
    thread_agents::update()
        .finished(true)
        .result(Some(result))
        .where_(
            thread_agents::agent_id
                .eq(agent_id)
                .and(thread_agents::thread_id.eq(thread_id)),
        )
        .execute(&db)
        .await
        .map_err(db_error)?;
    Ok(())
}

fn serialize_tool_calls(tool_calls: &[PartialToolCall]) -> Result<Option<String>, AgentPoolError> {
    let calls: Vec<_> = tool_calls
        .iter()
        .filter(|call| !call.name.is_empty())
        .map(|call| llm::ToolCallChunk {
            index: None,
            id: Some(call.id.clone()),
            call_type: Some("function".to_string()),
            function: Some(llm::ToolCallFunction {
                name: Some(call.name.clone()),
                arguments: Some(call.arguments.clone()),
            }),
        })
        .collect();

    if calls.is_empty() {
        return Ok(None);
    }

    serde_json::to_string(&calls)
        .map(Some)
        .map_err(|error| AgentPoolError::Internal(anyhow::anyhow!(error)))
}

fn with_format_reminder(content: String, format: MessageFormat) -> String {
    let reminder = match format {
        MessageFormat::Html => {
            "Reply in safe HTML only (p, ul/ol/li, table, h1-h6, strong, em, code, pre, blockquote, a, br, hr). \
             Markdown is NOT rendered on this surface: never write **bold**, [link](url), # headings, - bullets, \
             | tables |, or ``` fences. If you drafted Markdown, rewrite it as HTML before answering."
        }
        MessageFormat::Markdown => {
            "Reply in plain Telegram-friendly Markdown. Do not use HTML tags."
        }
    };
    format!("{content}\n\n<system-reminder>{reminder}</system-reminder>")
}

fn output_sanitizer(
    format: MessageFormat,
    state: &Rc<RefCell<WorkerState>>,
) -> Box<dyn StreamingMessageSanitizer> {
    match format {
        MessageFormat::Html => Box::new(HtmlFormattingSanitizer::new(
            state.borrow().init.public_url.clone(),
        )),
        MessageFormat::Markdown => Box::<RawMarkdownSanitizer>::default(),
    }
}

fn thread_message_format(
    state: &Rc<RefCell<WorkerState>>,
    thread_id: Uuid,
) -> Option<MessageFormat> {
    state
        .borrow()
        .threads
        .get(&thread_id)
        .map(|runner| runner.message_format)
}

async fn persist_tool_message(
    state: &Rc<RefCell<WorkerState>>,
    thread_id: Uuid,
    tool_call_id: &str,
    content: &str,
    agent_path: Option<&str>,
) -> Result<(), AgentPoolError> {
    let (db, id) = {
        let state = state.borrow();
        (
            state.init.db.clone(),
            state.init.config.id_gen.new_uuid_v7(),
        )
    };
    let seq = crate::runner::pool::next_message_seq(state, thread_id)?;

    messages::insert()
        .id(id)
        .parent_thread(thread_id)
        .seq(seq)
        .role(Role::Tool)
        .content(content)
        .content_format(MessageFormat::Markdown)
        .images(Option::<&str>::None)
        .thinking(Option::<&str>::None)
        .tool_calls(Option::<&str>::None)
        .tool_call_id(Some(tool_call_id))
        .agent_path(agent_path)
        .execute(&db)
        .await
        .map_err(db_error)?;

    Ok(())
}

fn restore_agent(
    state: &Rc<RefCell<WorkerState>>,
    thread_id: Uuid,
    agent: stride_agent::BaseAgent,
) {
    let clock = state.borrow().init.config.clock.clone();
    with_runner(state, thread_id, |runner| {
        runner.agent = Some(agent);
        runner.last_used = clock.now_instant();
    });
}

async fn fail_run(state: &Rc<RefCell<WorkerState>>, thread_id: Uuid, run_id: RunId, error: String) {
    let clock = state.borrow().init.config.clock.clone();
    with_runner(state, thread_id, |runner| {
        runner.cancel_tx = None;
        runner.broker.clear();
        runner.status = crate::runner::ThreadStatus::Idle;
        runner.in_progress = None;
        runner.last_used = clock.now_instant();
    });
    emit(
        state,
        thread_id,
        Some(run_id),
        AgentEventKind::RunFailed { error },
    )
    .await;
}

async fn cancel_run_task(state: &Rc<RefCell<WorkerState>>, thread_id: Uuid, run_id: RunId) {
    let clock = state.borrow().init.config.clock.clone();
    with_runner(state, thread_id, |runner| {
        runner.cancel_tx = None;
        runner.broker.clear();
        runner.status = crate::runner::ThreadStatus::Idle;
        runner.in_progress = None;
        runner.last_used = clock.now_instant();
    });
    emit(state, thread_id, Some(run_id), AgentEventKind::RunCancelled).await;
}

/// Stamps the event with the thread's next sequence number and publishes it to the thread's global
/// pub/sub topic. Every consumer (WS handler, Telegram subscriber) reads from that topic, whose
/// bounded backlog also serves reconnecting clients — so the worker only publishes and never owns
/// per-consumer fan-out state.
pub(crate) async fn emit(
    state: &Rc<RefCell<WorkerState>>,
    thread_id: Uuid,
    run_id: Option<RunId>,
    kind: AgentEventKind,
) {
    let Some(run_id) = run_id else {
        return;
    };
    let event = {
        let state = state.borrow();
        ThreadEvent {
            id: state.init.config.id_gen.new_uuid_v7(),
            run_id: run_id.0,
            agent_path: Vec::new(),
            kind,
        }
    };
    emit_thread_event(state, thread_id, event).await;
}

async fn emit_thread_event(state: &Rc<RefCell<WorkerState>>, thread_id: Uuid, shared: ThreadEvent) {
    let (event, db, global) = {
        let mut state = state.borrow_mut();
        let seq = {
            let Some(runner) = state.threads.get_mut(&thread_id) else {
                return;
            };
            runner.last_event_seq += 1;
            runner.last_event_seq
        };
        let db = state.init.db.clone();
        let global = state.threads.get(&thread_id).and_then(|runner| {
            if !shared.agent_path.is_empty() {
                return None;
            }
            let running = match &shared.kind {
                EventKind::RunStarted => Some(true),
                EventKind::RunFinished | EventKind::RunFailed { .. } | EventKind::RunCancelled => {
                    Some(false)
                }
                _ => None,
            }?;
            Some((runner.owner, running))
        });
        let event = AgentEvent {
            id: shared.id,
            seq,
            thread_id,
            run_id: Some(RunId(shared.run_id)),
            agent_path: shared.agent_path,
            kind: shared.kind,
        };
        (event, db, global)
    };

    let _ = pubsub::topic::<AgentEvent>(&thread_events_topic(thread_id)).publish(&event);
    if let Some((owner, running)) = global {
        crate::user_events::publish(
            owner,
            event.id,
            crate::user_events::UserEventKind::ThreadRunStatus { thread_id, running },
        );
    }
    journal_event(&db, &event).await;
}

/// Per-kind journal metadata: `(kind tag, message_id, terminal)`. Returns `None`
/// for deltas, which are never journaled — the committed message row holds the
/// final text and the snapshot carries in-flight partials.
fn journal_metadata(
    kind: &AgentEventKind,
) -> Option<(&'static str, Option<Uuid>, Option<&str>, bool)> {
    match kind {
        AgentEventKind::RunStarted => Some(("run_started", None, None, false)),
        AgentEventKind::RunFinished => Some(("run_finished", None, None, true)),
        AgentEventKind::RunFailed { .. } => Some(("run_failed", None, None, true)),
        AgentEventKind::RunCancelled => Some(("run_cancelled", None, None, true)),
        AgentEventKind::MessageStarted { message_id, .. } => {
            Some(("message_started", Some(*message_id), None, false))
        }
        AgentEventKind::MessageCommitted { message_id } => {
            Some(("message_committed", Some(*message_id), None, false))
        }
        AgentEventKind::ToolCallStarted { tool_call_id, .. } => {
            Some(("tool_call_started", None, Some(tool_call_id), false))
        }
        AgentEventKind::ToolCallProgress { tool_call_id, .. } => {
            Some(("tool_call_progress", None, Some(tool_call_id), false))
        }
        AgentEventKind::ToolCallFinished { tool_call_id, .. } => {
            Some(("tool_call_finished", None, Some(tool_call_id), false))
        }
        AgentEventKind::AgentSpawned { .. } => Some(("agent_spawned", None, None, false)),
        AgentEventKind::AgentFinished { .. } => Some(("agent_finished", None, None, false)),
        AgentEventKind::ApprovalRequested { tool_call_id, .. } => {
            Some(("approval_requested", None, Some(tool_call_id), false))
        }
        AgentEventKind::ApprovalResolved { .. } => Some(("approval_resolved", None, None, false)),
        AgentEventKind::QuizRequested { .. } => Some(("quiz_requested", None, None, false)),
        AgentEventKind::QuizAnswered { .. } => Some(("quiz_answered", None, None, false)),
        AgentEventKind::TextDelta { .. } | AgentEventKind::ThinkingDelta { .. } => None,
    }
}

/// Writes a structural event to the durable journal and prunes old runs once a
/// terminal event lands. Delta events and events without a run are skipped.
async fn journal_event(db: &ConnectionPool, event: &AgentEvent) {
    let Some((kind_tag, message_id, tool_call_id, terminal)) = journal_metadata(&event.kind) else {
        return;
    };
    let Some(run_id) = event.run_id else {
        return;
    };

    let payload = serde_json::to_string(&event.kind).ok();
    let agent_path = encode_agent_path(&event.agent_path);
    if kind_tag == "tool_call_progress"
        && let Some(tool_call_id) = tool_call_id
    {
        let _ = thread_events::delete()
            .where_(
                thread_events::thread_id
                    .eq(event.thread_id)
                    .and(thread_events::run_id.eq(run_id.0))
                    .and(thread_events::kind.eq("tool_call_progress"))
                    .and(thread_events::tool_call_id.eq(Some(tool_call_id))),
            )
            .execute(db)
            .await;
    }
    let result = thread_events::insert()
        .id(event.id)
        .thread_id(event.thread_id)
        .run_id(run_id.0)
        .seq(event.seq)
        .agent_path(agent_path.as_deref())
        .kind(kind_tag)
        .message_id(message_id)
        .tool_call_id(tool_call_id)
        .payload(payload.as_deref())
        .execute(db)
        .await;

    if let Err(error) = result {
        tracing::warn!(thread_id = %event.thread_id, %error, "failed to journal thread event");
        return;
    }

    if terminal {
        prune_journal(db, event.thread_id).await;
    }
}

fn encode_agent_path(path: &[Uuid]) -> Option<String> {
    (!path.is_empty()).then(|| {
        path.iter()
            .map(Uuid::to_string)
            .collect::<Vec<_>>()
            .join("/")
    })
}

fn decode_agent_path(path: Option<&str>) -> Vec<Uuid> {
    path.into_iter()
        .flat_map(|path| path.split('/'))
        .filter_map(|part| Uuid::parse_str(part).ok())
        .collect()
}

/// Keeps only the most recent [`JOURNAL_RETAINED_RUNS`] runs' events for a thread.
async fn prune_journal(db: &ConnectionPool, thread_id: Uuid) {
    let result = db
        .query_with_params(
            "DELETE FROM thread_events WHERE thread_id = ? AND run_id NOT IN ( \
               SELECT run_id FROM thread_events WHERE thread_id = ? \
               GROUP BY run_id ORDER BY MAX(seq) DESC LIMIT ?)",
            vec![
                Value::Uuid(thread_id),
                Value::Uuid(thread_id),
                Value::Integer(JOURNAL_RETAINED_RUNS as i64),
            ],
        )
        .await;
    if let Err(error) = result {
        tracing::warn!(%thread_id, %error, "failed to prune thread event journal");
    }
}

/// Highest journaled `seq` for a thread, or 0 when the journal is empty. Used to
/// (re)initialize a worker's per-thread counter so it never resets across runner
/// recreation — every structural event, including the terminal one of each run,
/// is journaled, so at rest this equals the last emitted seq.
pub(crate) async fn load_last_event_seq(db: &ConnectionPool, thread_id: Uuid) -> u64 {
    let result = db
        .query_with_params(
            "SELECT MAX(seq) AS max_seq FROM thread_events WHERE thread_id = ?",
            vec![Value::Uuid(thread_id)],
        )
        .await;
    match result {
        Ok(rows) => rows
            .rows()
            .first()
            .and_then(|row| row.get_int("max_seq"))
            .and_then(|seq| u64::try_from(seq).ok())
            .unwrap_or(0),
        Err(error) => {
            tracing::warn!(%thread_id, %error, "failed to load last event seq; starting at 0");
            0
        }
    }
}

/// Reads journaled structural events with `seq` greater than `after`, in seq
/// order, reconstructing [`AgentEvent`]s from the stored payload so a
/// reconnecting consumer replays identical frames.
pub(crate) async fn journal_events_after(
    db: &ConnectionPool,
    thread_id: Uuid,
    after: u64,
) -> Vec<AgentEvent> {
    let result = db
        .query_with_params(
            "SELECT id, seq, run_id, agent_path, payload FROM thread_events \
             WHERE thread_id = ? AND seq > ? ORDER BY seq ASC",
            vec![Value::Uuid(thread_id), Value::Integer(after as i64)],
        )
        .await;
    let rows = match result {
        Ok(rows) => rows,
        Err(error) => {
            tracing::warn!(%thread_id, %error, "failed to read thread event journal");
            return Vec::new();
        }
    };

    let mut events = Vec::new();
    for row in rows.rows() {
        let Some(id) = row_uuid(row, "id") else {
            continue;
        };
        let Some(seq) = row.get_int("seq").and_then(|seq| u64::try_from(seq).ok()) else {
            continue;
        };
        let run_id = match row.get("run_id") {
            Some(Value::Uuid(id)) => Some(RunId(*id)),
            Some(Value::Blob(bytes)) if bytes.len() == 16 => {
                Uuid::from_slice(bytes).ok().map(RunId)
            }
            Some(Value::Text(text)) => Uuid::parse_str(text).ok().map(RunId),
            _ => None,
        };
        let Some(payload) = row.get_text("payload") else {
            continue;
        };
        let Ok(kind) = serde_json::from_str::<AgentEventKind>(payload) else {
            continue;
        };
        events.push(AgentEvent {
            id,
            seq,
            thread_id,
            run_id,
            agent_path: decode_agent_path(row.get_text("agent_path")),
            kind,
        });
    }
    events
}

fn row_uuid(row: &minisql::Row, column: &str) -> Option<Uuid> {
    match row.get(column) {
        Some(Value::Uuid(id)) => Some(*id),
        Some(Value::Blob(bytes)) if bytes.len() == 16 => Uuid::from_slice(bytes).ok(),
        Some(Value::Text(text)) => Uuid::parse_str(text).ok(),
        _ => None,
    }
}

async fn update_message(
    db: &minisql::ConnectionPool,
    id: Uuid,
    content: &str,
    thinking: Option<&str>,
    tool_calls: Option<&str>,
) -> Result<(), AgentPoolError> {
    messages::update()
        .content(content)
        .thinking(thinking)
        .tool_calls(tool_calls)
        .where_(messages::id.eq(id))
        .execute(db)
        .await
        .map_err(db_error)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, time::Duration};

    use llm::{CompletionChoice, Delta, StreamResponseChunk, ToolCallChunk, ToolCallFunction};
    use minisql::{ConnectionPool, Value};
    use stride_agent::{AgentConfig, DEFAULT_MODEL, ModelRegEntry, ModelRegistry};

    use super::*;
    use crate::config;
    use crate::crypto::SecretCipher;
    use crate::db::{self, threads, users};
    use crate::runner::pool::InProcessAgentPool;
    use crate::runner::{
        AgentEvent, AgentPool, AgentRequest, ThreadStatus, bootstrap::load_thread,
    };

    fn subscribe_events(thread_id: Uuid) -> pubsub::Subscriber<AgentEvent> {
        pubsub::topic::<AgentEvent>(&thread_events_topic(thread_id)).subscribe()
    }

    fn test_server_config() -> config::Config {
        config::Config {
            providers: HashMap::new(),
            models: HashMap::new(),
            server: None,
            tools: None,
            mcp: HashMap::new(),
        }
    }

    fn test_pool(db: ConnectionPool, models: ModelRegistry) -> InProcessAgentPool {
        InProcessAgentPool::builder(
            db,
            std::sync::Arc::new(AgentConfig {
                model_registry: models,
                max_iterations: 4,
                usage_observer: std::sync::Arc::new(stride_agent::NoopUsageObserver),
                ..Default::default()
            }),
            test_server_config(),
            SecretCipher::new("test-secret"),
        )
        .system_prompt("System prompt")
        .idle_ttl(Duration::from_secs(60))
        .build()
    }

    /// Pool with deterministic clock and id generator so event ids and timestamps
    /// are reproducible; seq assertions stay stable regardless.
    fn deterministic_pool(db: ConnectionPool, models: ModelRegistry) -> InProcessAgentPool {
        use stride_agent::determinism::{SeededIdGen, TestClock};
        InProcessAgentPool::builder(
            db,
            std::sync::Arc::new(AgentConfig {
                model_registry: models,
                max_iterations: 4,
                usage_observer: std::sync::Arc::new(stride_agent::NoopUsageObserver),
                clock: std::sync::Arc::new(TestClock::new(1_700_000_000_000)),
                id_gen: std::sync::Arc::new(SeededIdGen::new(7)),
                max_concurrent_tools: 4,
            }),
            test_server_config(),
            SecretCipher::new("test-secret"),
        )
        .system_prompt("System prompt")
        .idle_ttl(Duration::from_secs(60))
        .build()
    }

    async fn seed_owner_and_thread(db: &ConnectionPool, owner: Uuid, thread_id: Uuid) {
        users::insert()
            .id(owner)
            .username(format!("u-{}", owner.as_simple()).as_str())
            .password_hash("hash")
            .execute(db)
            .await
            .unwrap();
        threads::insert()
            .id(thread_id)
            .owner(owner)
            .title("Test")
            .execute(db)
            .await
            .unwrap();
    }

    fn single_reply_registry(text: &str) -> ModelRegistry {
        let mut models = ModelRegistry::new();
        models.add_model(
            DEFAULT_MODEL,
            ModelRegEntry {
                api: llm::Mock::new()
                    .with_stream_chunks(vec![vec![text_chunk(text)]])
                    .into(),
                token: "-".to_string(),
                model_name: "mock-model".to_string(),
                reasoning_effort: None,
                vision: false,
            },
        );
        models
    }

    async fn run_to_completion(pool: &InProcessAgentPool, thread_id: Uuid, content: &str) {
        pool.send(
            thread_id,
            AgentRequest {
                content: content.to_string(),
                images: Vec::new(),
                model: None,
            },
        )
        .await
        .unwrap();
        while pool.status(thread_id).await.unwrap() != ThreadStatus::Idle {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    #[tokio::test]
    async fn send_persists_messages_and_streams_events() {
        let db = ConnectionPool::new("sqlite::memory:").unwrap();
        db.initialize_database(db::get_migrations()).await.unwrap();

        let owner = Uuid::now_v7();
        let thread_id = Uuid::now_v7();

        users::insert()
            .id(owner)
            .username("alice")
            .password_hash("hash")
            .execute(&db)
            .await
            .unwrap();
        threads::insert()
            .id(thread_id)
            .owner(owner)
            .title("Test")
            .execute(&db)
            .await
            .unwrap();

        let mut models = ModelRegistry::new();
        models.add_model(
            DEFAULT_MODEL,
            ModelRegEntry {
                api: llm::Mock::new()
                    .with_stream_chunks(vec![vec![text_chunk("pong")]])
                    .into(),
                token: "-".to_string(),
                model_name: "mock-model".to_string(),
                reasoning_effort: None,
                vision: false,
            },
        );

        let pool = test_pool(db.clone(), models);

        let mut subscription = subscribe_events(thread_id);
        let mut user_events =
            pubsub::topic::<crate::user_events::UserEvent>(&crate::user_events::topic(owner))
                .subscribe();
        let run_id = pool
            .send(
                thread_id,
                AgentRequest {
                    content: "ping".to_string(),
                    images: Vec::new(),
                    model: None,
                },
            )
            .await
            .unwrap();

        let mut saw_delta = false;
        let mut saw_finished = false;
        for _ in 0..8 {
            let event = tokio::time::timeout(Duration::from_secs(2), subscription.recv())
                .await
                .unwrap()
                .unwrap();

            assert_eq!(event.thread_id, thread_id);
            assert_eq!(event.run_id, Some(run_id));

            match event.kind {
                AgentEventKind::TextDelta { delta, .. } if delta == "pong" => {
                    saw_delta = true;
                }
                AgentEventKind::RunFinished => {
                    saw_finished = true;
                    break;
                }
                _ => {}
            }
        }

        assert!(saw_delta);
        assert!(saw_finished);
        assert_eq!(pool.status(thread_id).await.unwrap(), ThreadStatus::Idle);
        let mut statuses = Vec::new();
        while statuses.len() < 2 {
            let event = tokio::time::timeout(Duration::from_secs(2), user_events.recv())
                .await
                .unwrap()
                .unwrap();
            if let crate::user_events::UserEventKind::ThreadRunStatus { running, .. } = event.kind {
                statuses.push(running);
            }
        }
        assert_eq!(statuses, vec![true, false]);

        let rows = db
            .query_with_params(
                "SELECT role, content FROM messages WHERE parent_thread = ? ORDER BY seq ASC",
                vec![Value::Uuid(thread_id)],
            )
            .await
            .unwrap();
        let rows = rows.rows();

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].get_text("role"), Some("user"));
        assert_eq!(rows[0].get_text("content"), Some("ping"));
        assert_eq!(rows[1].get_text("role"), Some("agent"));
        assert_eq!(rows[1].get_text("content"), Some("pong"));
    }

    #[tokio::test]
    async fn send_uses_requested_model() {
        let db = ConnectionPool::new("sqlite::memory:").unwrap();
        db.initialize_database(db::get_migrations()).await.unwrap();

        let owner = Uuid::now_v7();
        let thread_id = Uuid::now_v7();

        users::insert()
            .id(owner)
            .username("alice")
            .password_hash("hash")
            .execute(&db)
            .await
            .unwrap();
        threads::insert()
            .id(thread_id)
            .owner(owner)
            .title("Test")
            .execute(&db)
            .await
            .unwrap();

        let default_mock = llm::Mock::new().with_stream_chunks(vec![vec![text_chunk("default")]]);
        let selected_mock = llm::Mock::new().with_stream_chunks(vec![vec![text_chunk("selected")]]);
        let mut models = ModelRegistry::new();
        models.add_model(
            DEFAULT_MODEL,
            ModelRegEntry {
                api: default_mock.clone().into(),
                token: "-".to_string(),
                model_name: "default-upstream".to_string(),
                reasoning_effort: None,
                vision: false,
            },
        );
        models.add_model(
            "fast",
            ModelRegEntry {
                api: selected_mock.clone().into(),
                token: "-".to_string(),
                model_name: "fast-upstream".to_string(),
                reasoning_effort: None,
                vision: false,
            },
        );

        let pool = test_pool(db.clone(), models);
        let mut subscription = subscribe_events(thread_id);
        pool.send(
            thread_id,
            AgentRequest {
                content: "ping".to_string(),
                images: Vec::new(),
                model: Some("fast".to_string()),
            },
        )
        .await
        .unwrap();

        for _ in 0..8 {
            let event = tokio::time::timeout(Duration::from_secs(2), subscription.recv())
                .await
                .unwrap()
                .unwrap();
            if matches!(event.kind, AgentEventKind::RunFinished) {
                break;
            }
        }

        assert!(default_mock.stream_requests().is_empty());
        let requests = selected_mock.stream_requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].model, "fast-upstream");

        let stored = threads::select_cols((threads::last_model,))
            .where_(threads::id.eq(thread_id))
            .all(&db)
            .await
            .unwrap()
            .into_iter()
            .next()
            .and_then(|(model,)| model);
        assert_eq!(stored.as_deref(), Some("fast"));
    }

    #[tokio::test]
    async fn send_persists_tool_calls_and_outputs() {
        let db = ConnectionPool::new("sqlite::memory:").unwrap();
        db.initialize_database(db::get_migrations()).await.unwrap();

        let owner = Uuid::now_v7();
        let thread_id = Uuid::now_v7();

        users::insert()
            .id(owner)
            .username("alice")
            .password_hash("hash")
            .execute(&db)
            .await
            .unwrap();
        threads::insert()
            .id(thread_id)
            .owner(owner)
            .title("Test")
            .execute(&db)
            .await
            .unwrap();

        let mut models = ModelRegistry::new();
        models.add_model(
            DEFAULT_MODEL,
            ModelRegEntry {
                api: llm::Mock::new()
                    .with_stream_chunks(vec![
                        vec![tool_call_chunk("call-1", "missing_tool", r#"{"value":1}"#)],
                        vec![text_chunk("done")],
                    ])
                    .into(),
                token: "-".to_string(),
                model_name: "mock-model".to_string(),
                reasoning_effort: None,
                vision: false,
            },
        );

        let pool = test_pool(db.clone(), models);

        let mut subscription = subscribe_events(thread_id);
        pool.send(
            thread_id,
            AgentRequest {
                content: "run tool".to_string(),
                images: Vec::new(),
                model: None,
            },
        )
        .await
        .unwrap();

        let mut saw_tool_started = false;
        let mut saw_tool_finished = false;
        let mut saw_finished = false;
        for _ in 0..12 {
            let event = tokio::time::timeout(Duration::from_secs(2), subscription.recv())
                .await
                .unwrap()
                .unwrap();

            match event.kind {
                AgentEventKind::ToolCallStarted { name, .. } if name == "missing_tool" => {
                    saw_tool_started = true;
                }
                AgentEventKind::ToolCallFinished { name, .. } if name == "missing_tool" => {
                    saw_tool_finished = true;
                }
                AgentEventKind::RunFinished => {
                    saw_finished = true;
                    break;
                }
                _ => {}
            }
        }

        assert!(saw_tool_started);
        assert!(saw_tool_finished);
        assert!(saw_finished);

        let rows = db
            .query_with_params(
                "SELECT role, content, tool_calls, tool_call_id FROM messages WHERE parent_thread = ? ORDER BY seq ASC",
                vec![Value::Uuid(thread_id)],
            )
            .await
            .unwrap();
        let rows = rows.rows();

        assert_eq!(rows.len(), 4);
        assert_eq!(rows[1].get_text("role"), Some("agent"));
        assert_eq!(rows[1].get_text("content"), Some(""));
        assert!(rows[1].get_text("tool_calls").is_some());
        assert_eq!(rows[2].get_text("role"), Some("tool"));
        assert_eq!(rows[2].get_text("tool_call_id"), Some("call-1"));
        assert!(
            rows[2]
                .get_text("content")
                .unwrap()
                .contains("unknown tool")
        );
        assert_eq!(rows[3].get_text("role"), Some("agent"));
        assert_eq!(rows[3].get_text("content"), Some("done"));

        let (thread, _) = load_thread(&db, thread_id).await.unwrap();
        assert_eq!(thread[1].tool_calls.as_ref().unwrap().len(), 1);
        assert_eq!(thread[2].tool_call_id.as_deref(), Some("call-1"));
    }

    #[tokio::test]
    async fn send_sanitizes_streamed_agent_html() {
        let db = ConnectionPool::new("sqlite::memory:").unwrap();
        db.initialize_database(db::get_migrations()).await.unwrap();

        let owner = Uuid::now_v7();
        let thread_id = Uuid::now_v7();

        users::insert()
            .id(owner)
            .username("alice")
            .password_hash("hash")
            .execute(&db)
            .await
            .unwrap();
        threads::insert()
            .id(thread_id)
            .owner(owner)
            .title("Test")
            .execute(&db)
            .await
            .unwrap();

        let mut models = ModelRegistry::new();
        models.add_model(
            DEFAULT_MODEL,
            ModelRegEntry {
                api: llm::Mock::new()
                    .with_stream_chunks(vec![vec![
                        text_stream_chunk("<h1", None),
                        text_stream_chunk(r#">Hello<script>alert(1)</script>"#, Some("stop")),
                    ]])
                    .into(),
                token: "-".to_string(),
                model_name: "mock-model".to_string(),
                reasoning_effort: None,
                vision: false,
            },
        );

        let pool = test_pool(db.clone(), models);

        let mut subscription = subscribe_events(thread_id);
        pool.send(
            thread_id,
            AgentRequest {
                content: "ping".to_string(),
                images: Vec::new(),
                model: None,
            },
        )
        .await
        .unwrap();

        let mut saw_speculative_html = false;
        for _ in 0..8 {
            let event = tokio::time::timeout(Duration::from_secs(2), subscription.recv())
                .await
                .unwrap()
                .unwrap();
            match event.kind {
                AgentEventKind::TextDelta { delta, .. } if delta.contains("Hello") => {
                    saw_speculative_html = true;
                }
                AgentEventKind::RunFinished => break,
                _ => {}
            }
        }
        assert!(saw_speculative_html);

        let rows = db
            .query_with_params(
                "SELECT role, content, content_format FROM messages WHERE parent_thread = ? ORDER BY seq ASC",
                vec![Value::Uuid(thread_id)],
            )
            .await
            .unwrap();
        let rows = rows.rows();

        assert_eq!(rows[1].get_text("role"), Some("agent"));
        assert_eq!(rows[1].get_text("content"), Some("<h1>Helloalert(1)</h1>"));
        assert_eq!(rows[1].get_text("content_format"), Some("html"));
    }

    #[tokio::test]
    async fn send_ignores_empty_stream_deltas() {
        let db = ConnectionPool::new("sqlite::memory:").unwrap();
        db.initialize_database(db::get_migrations()).await.unwrap();

        let owner = Uuid::now_v7();
        let thread_id = Uuid::now_v7();

        users::insert()
            .id(owner)
            .username("alice")
            .password_hash("hash")
            .execute(&db)
            .await
            .unwrap();
        threads::insert()
            .id(thread_id)
            .owner(owner)
            .title("Test")
            .execute(&db)
            .await
            .unwrap();

        let mut models = ModelRegistry::new();
        models.add_model(
            DEFAULT_MODEL,
            ModelRegEntry {
                api: llm::Mock::new()
                    .with_stream_chunks(vec![vec![empty_delta_chunk(), text_chunk("pong")]])
                    .into(),
                token: "-".to_string(),
                model_name: "mock-model".to_string(),
                reasoning_effort: None,
                vision: false,
            },
        );

        let pool = test_pool(db.clone(), models);

        pool.send(
            thread_id,
            AgentRequest {
                content: "ping".to_string(),
                images: Vec::new(),
                model: None,
            },
        )
        .await
        .unwrap();

        while pool.status(thread_id).await.unwrap() != ThreadStatus::Idle {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        let rows = db
            .query_with_params(
                "SELECT role, content, thinking, tool_calls, tool_call_id FROM messages WHERE parent_thread = ? ORDER BY seq ASC",
                vec![Value::Uuid(thread_id)],
            )
            .await
            .unwrap();
        let rows = rows.rows();

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].get_text("role"), Some("user"));
        assert_eq!(rows[1].get_text("role"), Some("agent"));
        assert_eq!(rows[1].get_text("content"), Some("pong"));
        assert!(matches!(rows[1].get("thinking"), Some(Value::Null) | None));
        assert!(matches!(
            rows[1].get("tool_calls"),
            Some(Value::Null) | None
        ));
        assert!(matches!(
            rows[1].get("tool_call_id"),
            Some(Value::Null) | None
        ));
    }

    #[tokio::test]
    async fn send_persists_full_choice_messages() {
        let db = ConnectionPool::new("sqlite::memory:").unwrap();
        db.initialize_database(db::get_migrations()).await.unwrap();

        let owner = Uuid::now_v7();
        let thread_id = Uuid::now_v7();

        users::insert()
            .id(owner)
            .username("alice")
            .password_hash("hash")
            .execute(&db)
            .await
            .unwrap();
        threads::insert()
            .id(thread_id)
            .owner(owner)
            .title("Test")
            .execute(&db)
            .await
            .unwrap();

        let mut models = ModelRegistry::new();
        models.add_model(
            DEFAULT_MODEL,
            ModelRegEntry {
                api: llm::Mock::new()
                    .with_stream_chunks(vec![vec![message_chunk("think", "pong")]])
                    .into(),
                token: "-".to_string(),
                model_name: "mock-model".to_string(),
                reasoning_effort: None,
                vision: false,
            },
        );

        let pool = test_pool(db.clone(), models);

        pool.send(
            thread_id,
            AgentRequest {
                content: "ping".to_string(),
                images: Vec::new(),
                model: None,
            },
        )
        .await
        .unwrap();

        while pool.status(thread_id).await.unwrap() != ThreadStatus::Idle {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        let rows = db
            .query_with_params(
                "SELECT role, content, thinking FROM messages WHERE parent_thread = ? ORDER BY seq ASC",
                vec![Value::Uuid(thread_id)],
            )
            .await
            .unwrap();
        let rows = rows.rows();

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].get_text("role"), Some("agent"));
        assert_eq!(rows[1].get_text("content"), Some("pong"));
        assert_eq!(rows[1].get_text("thinking"), Some("think"));
    }

    fn text_chunk(content: &str) -> StreamResponseChunk {
        text_stream_chunk(content, Some("stop"))
    }

    fn text_stream_chunk(content: &str, finish_reason: Option<&str>) -> StreamResponseChunk {
        StreamResponseChunk {
            id: "mock-stream-id".to_string(),
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
                    content: Some(content.to_string()),
                    thinking: None,
                    tool_calls: None,
                }),
                logprobs: None,
                tool_calls: None,
                finish_reason: finish_reason.map(str::to_string),
            }],
        }
    }

    fn empty_delta_chunk() -> StreamResponseChunk {
        StreamResponseChunk {
            id: "mock-stream-id".to_string(),
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
                    content: Some(String::new()),
                    thinking: Some(String::new()),
                    tool_calls: None,
                }),
                logprobs: None,
                tool_calls: None,
                finish_reason: None,
            }],
        }
    }

    fn message_chunk(thinking: &str, content: &str) -> StreamResponseChunk {
        StreamResponseChunk {
            id: "mock-stream-id".to_string(),
            object: "mock.stream".to_string(),
            created: 0,
            model: "mock-model".to_string(),
            system_fingerprint: None,
            usage: None,
            choices: vec![CompletionChoice {
                message: Some(llm::Message {
                    role: llm::Role::Assistant,
                    content: content.to_string(),
                    thinking: Some(thinking.to_string()),
                    ..Default::default()
                }),
                text: None,
                index: 0,
                delta: None,
                logprobs: None,
                tool_calls: None,
                finish_reason: Some("stop".to_string()),
            }],
        }
    }

    fn tool_call_chunk(id: &str, name: &str, arguments: &str) -> StreamResponseChunk {
        StreamResponseChunk {
            id: "mock-stream-id".to_string(),
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

    #[tokio::test]
    async fn late_subscriber_replays_backlog_within_snapshot_watermark() {
        let db = ConnectionPool::new("sqlite::memory:").unwrap();
        db.initialize_database(db::get_migrations()).await.unwrap();

        let owner = Uuid::now_v7();
        let thread_id = Uuid::now_v7();

        users::insert()
            .id(owner)
            .username("alice")
            .password_hash("hash")
            .execute(&db)
            .await
            .unwrap();
        threads::insert()
            .id(thread_id)
            .owner(owner)
            .title("Test")
            .execute(&db)
            .await
            .unwrap();

        let mut models = ModelRegistry::new();
        models.add_model(
            DEFAULT_MODEL,
            ModelRegEntry {
                api: llm::Mock::new()
                    .with_stream_chunks(vec![vec![text_chunk("pong")]])
                    .into(),
                token: "-".to_string(),
                model_name: "mock-model".to_string(),
                reasoning_effort: None,
                vision: false,
            },
        );

        let pool = test_pool(db.clone(), models);

        // Run to completion.
        pool.send(
            thread_id,
            AgentRequest {
                content: "ping".to_string(),
                images: Vec::new(),
                model: None,
            },
        )
        .await
        .unwrap();
        while pool.status(thread_id).await.unwrap() != ThreadStatus::Idle {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        // A late subscriber replays the topic's bounded backlog before any live events.
        let snapshot = pool.snapshot(thread_id).await.unwrap();
        let mut sub = subscribe_events(thread_id);
        let mut replayed = Vec::new();
        while let Ok(Ok(event)) = tokio::time::timeout(Duration::from_millis(100), sub.recv()).await
        {
            replayed.push(event);
        }
        assert!(
            replayed
                .iter()
                .any(|e| matches!(e.kind, AgentEventKind::RunFinished)),
            "backlog replay must include RunFinished"
        );
        // Every replayed event is at or below the snapshot watermark, so a consumer that gates on
        // last_event_seq (as the WS handler does) discards them all and never double-applies.
        assert!(
            replayed.iter().all(|e| e.seq <= snapshot.last_event_seq),
            "replayed events must not exceed the snapshot watermark"
        );
    }

    #[tokio::test]
    async fn cancel_run_terminates_cleanly() {
        let db = ConnectionPool::new("sqlite::memory:").unwrap();
        db.initialize_database(db::get_migrations()).await.unwrap();

        let owner = Uuid::now_v7();
        let thread_id = Uuid::now_v7();

        users::insert()
            .id(owner)
            .username("alice")
            .password_hash("hash")
            .execute(&db)
            .await
            .unwrap();
        threads::insert()
            .id(thread_id)
            .owner(owner)
            .title("Test")
            .execute(&db)
            .await
            .unwrap();

        let mut models = ModelRegistry::new();
        models.add_model(
            DEFAULT_MODEL,
            ModelRegEntry {
                api: llm::Mock::new()
                    .with_stream_chunks(vec![vec![text_chunk("pong")]])
                    .into(),
                token: "-".to_string(),
                model_name: "mock-model".to_string(),
                reasoning_effort: None,
                vision: false,
            },
        );

        let pool = test_pool(db.clone(), models);

        let mut sub = subscribe_events(thread_id);
        pool.send(
            thread_id,
            AgentRequest {
                content: "ping".to_string(),
                images: Vec::new(),
                model: None,
            },
        )
        .await
        .unwrap();
        pool.cancel_run(thread_id).await.unwrap();

        let mut terminated = false;
        for _ in 0..12 {
            let event = tokio::time::timeout(Duration::from_secs(2), sub.recv())
                .await
                .unwrap()
                .unwrap();
            match event.kind {
                AgentEventKind::RunCancelled | AgentEventKind::RunFinished => {
                    terminated = true;
                    break;
                }
                _ => {}
            }
        }

        assert!(terminated, "run must terminate (cancelled or finished)");
        assert_eq!(pool.status(thread_id).await.unwrap(), ThreadStatus::Idle);
    }

    #[tokio::test]
    async fn quiz_answer_through_pool_completes_run() {
        let db = ConnectionPool::new("sqlite::memory:").unwrap();
        db.initialize_database(db::get_migrations()).await.unwrap();

        let owner = Uuid::now_v7();
        let thread_id = Uuid::now_v7();
        users::insert()
            .id(owner)
            .username("alice")
            .password_hash("hash")
            .execute(&db)
            .await
            .unwrap();
        threads::insert()
            .id(thread_id)
            .owner(owner)
            .title("Test")
            .execute(&db)
            .await
            .unwrap();

        let mut models = ModelRegistry::new();
        models.add_model(
            DEFAULT_MODEL,
            ModelRegEntry {
                api: llm::Mock::new()
                    .with_stream_chunks(vec![
                        vec![tool_call_chunk(
                            "call-1",
                            "quiz",
                            r#"{"questions":[{"question":"Pick","options":["a","b"]}]}"#,
                        )],
                        vec![text_chunk("done")],
                    ])
                    .into(),
                token: "-".to_string(),
                model_name: "mock-model".to_string(),
                reasoning_effort: None,
                vision: false,
            },
        );

        let pool = test_pool(db.clone(), models);

        let mut sub = subscribe_events(thread_id);
        pool.send(
            thread_id,
            AgentRequest {
                content: "ask".to_string(),
                images: Vec::new(),
                model: None,
            },
        )
        .await
        .unwrap();

        let mut quiz_id = None;
        for _ in 0..20 {
            let event = tokio::time::timeout(Duration::from_secs(2), sub.recv())
                .await
                .unwrap()
                .unwrap();
            if let AgentEventKind::QuizRequested { quiz_id: id, .. } = event.kind {
                quiz_id = Some(id);
                break;
            }
        }
        let quiz_id = quiz_id.expect("agent must present the quiz");

        // The tap path: resolve the pending quiz through the pool while the run is waiting.
        pool.answer_quiz(thread_id, quiz_id, vec!["a".to_string()])
            .await
            .unwrap();

        let mut finished = false;
        for _ in 0..20 {
            let event = tokio::time::timeout(Duration::from_secs(2), sub.recv())
                .await
                .unwrap()
                .unwrap();
            if matches!(event.kind, AgentEventKind::RunFinished) {
                finished = true;
                break;
            }
        }
        assert!(finished, "run must complete after the quiz is answered");
        assert_eq!(pool.status(thread_id).await.unwrap(), ThreadStatus::Idle);
    }

    #[tokio::test]
    async fn slow_subscriber_does_not_block_worker_commands() {
        let db = ConnectionPool::new("sqlite::memory:").unwrap();
        db.initialize_database(db::get_migrations()).await.unwrap();

        let owner = Uuid::now_v7();
        let thread_id = Uuid::now_v7();
        users::insert()
            .id(owner)
            .username("alice")
            .password_hash("hash")
            .execute(&db)
            .await
            .unwrap();
        threads::insert()
            .id(thread_id)
            .owner(owner)
            .title("Test")
            .execute(&db)
            .await
            .unwrap();

        let mut models = ModelRegistry::new();
        models.add_model(
            DEFAULT_MODEL,
            ModelRegEntry {
                api: llm::Mock::new()
                    .with_stream_chunks(vec![vec![text_chunk("pong")]])
                    .into(),
                token: "-".to_string(),
                model_name: "mock-model".to_string(),
                reasoning_effort: None,
                vision: false,
            },
        );

        let pool = test_pool(db, models);

        // A deliberately slow consumer of the thread's events. Publishing is decoupled from
        // consumers, so this must not slow down worker commands.
        let mut slow = subscribe_events(thread_id);
        tokio::spawn(async move {
            while slow.recv().await.is_ok() {
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        });

        tokio::time::timeout(
            Duration::from_millis(100),
            pool.send(
                thread_id,
                AgentRequest {
                    content: "ping".to_string(),
                    images: Vec::new(),
                    model: None,
                },
            ),
        )
        .await
        .expect("send must not wait for a slow subscriber")
        .unwrap();

        tokio::time::timeout(Duration::from_millis(100), pool.status(thread_id))
            .await
            .expect("status must not wait for a slow subscriber")
            .unwrap();
    }

    #[tokio::test]
    async fn pubsub_subscriber_receives_run_events() {
        let db = ConnectionPool::new("sqlite::memory:").unwrap();
        db.initialize_database(db::get_migrations()).await.unwrap();

        let owner = Uuid::now_v7();
        let thread_id = Uuid::now_v7();
        users::insert()
            .id(owner)
            .username("alice")
            .password_hash("hash")
            .execute(&db)
            .await
            .unwrap();
        threads::insert()
            .id(thread_id)
            .owner(owner)
            .title("Test")
            .execute(&db)
            .await
            .unwrap();

        let mut models = ModelRegistry::new();
        models.add_model(
            DEFAULT_MODEL,
            ModelRegEntry {
                api: llm::Mock::new()
                    .with_stream_chunks(vec![vec![text_chunk("pong")]])
                    .into(),
                token: "-".to_string(),
                model_name: "mock-model".to_string(),
                reasoning_effort: None,
                vision: false,
            },
        );

        let pool = test_pool(db.clone(), models);

        let mut sub = subscribe_events(thread_id);
        pool.send(
            thread_id,
            AgentRequest {
                content: "ping".to_string(),
                images: Vec::new(),
                model: None,
            },
        )
        .await
        .unwrap();

        // A pub/sub subscriber must see every event of the run, end to end.
        let mut saw_started = false;
        let mut saw_delta = false;
        let mut saw_finished = false;
        for _ in 0..50 {
            let Ok(Ok(event)) = tokio::time::timeout(Duration::from_secs(2), sub.recv()).await
            else {
                break;
            };
            match event.kind {
                AgentEventKind::RunStarted => saw_started = true,
                AgentEventKind::TextDelta { .. } => saw_delta = true,
                AgentEventKind::RunFinished => {
                    saw_finished = true;
                    break;
                }
                _ => {}
            }
        }

        assert!(saw_started, "subscriber must receive RunStarted");
        assert!(saw_delta, "subscriber must receive AgentDelta");
        assert!(saw_finished, "subscriber must receive RunFinished");
    }

    fn count_run_finished(events: &[AgentEvent]) -> usize {
        events
            .iter()
            .filter(|e| matches!(e.kind, AgentEventKind::RunFinished))
            .count()
    }

    async fn wait_for_finished_runs(db: &ConnectionPool, thread_id: Uuid, want: usize) {
        for _ in 0..200 {
            let events = super::journal_events_after(db, thread_id, 0).await;
            if count_run_finished(&events) >= want {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("journal never reached {want} finished runs");
    }

    // R1 regression: after the runner is evicted and recreated, the per-thread seq must keep
    // climbing from the journal's max, not reset to 0 (which the client would silently drop).
    #[tokio::test]
    async fn seq_is_monotonic_across_runner_recreation() {
        let db = ConnectionPool::new("sqlite::memory:").unwrap();
        db.initialize_database(db::get_migrations()).await.unwrap();

        let owner = Uuid::now_v7();
        let thread_id = Uuid::now_v7();
        seed_owner_and_thread(&db, owner, thread_id).await;

        let mut models = ModelRegistry::new();
        models.add_model(
            DEFAULT_MODEL,
            ModelRegEntry {
                api: llm::Mock::new()
                    .with_stream_chunks(vec![vec![text_chunk("one")], vec![text_chunk("two")]])
                    .into(),
                token: "-".to_string(),
                model_name: "mock-model".to_string(),
                reasoning_effort: None,
                vision: false,
            },
        );
        let pool = deterministic_pool(db.clone(), models);

        run_to_completion(&pool, thread_id, "first").await;
        wait_for_finished_runs(&db, thread_id, 1).await;
        let first = super::journal_events_after(&db, thread_id, 0).await;
        let max_after_first = first.iter().map(|e| e.seq).max().unwrap();
        assert!(max_after_first > 0);
        assert_eq!(
            super::load_last_event_seq(&db, thread_id).await,
            max_after_first
        );

        // Evict the runner; the next request recreates it from scratch.
        pool.shutdown_thread(thread_id).await.unwrap();

        run_to_completion(&pool, thread_id, "second").await;
        wait_for_finished_runs(&db, thread_id, 2).await;
        let all = super::journal_events_after(&db, thread_id, 0).await;

        let seqs: Vec<u64> = all.iter().map(|e| e.seq).collect();
        assert!(
            seqs.windows(2).all(|w| w[0] < w[1]),
            "journal seqs must be strictly increasing: {seqs:?}"
        );
        let second_min = all
            .iter()
            .map(|e| e.seq)
            .filter(|seq| *seq > max_after_first)
            .min()
            .unwrap();
        assert_eq!(
            second_min,
            max_after_first + 1,
            "recreated runner must continue seq from the journal, not reset"
        );
    }

    // 5b: only structural events are journaled (no deltas) and they round-trip through the DB.
    #[tokio::test]
    async fn journal_round_trip_stores_structural_events_only() {
        let db = ConnectionPool::new("sqlite::memory:").unwrap();
        db.initialize_database(db::get_migrations()).await.unwrap();

        let owner = Uuid::now_v7();
        let thread_id = Uuid::now_v7();
        seed_owner_and_thread(&db, owner, thread_id).await;

        let pool = deterministic_pool(db.clone(), single_reply_registry("pong"));
        run_to_completion(&pool, thread_id, "ping").await;
        wait_for_finished_runs(&db, thread_id, 1).await;

        let events = super::journal_events_after(&db, thread_id, 0).await;
        assert!(
            events.iter().all(|e| !matches!(
                e.kind,
                AgentEventKind::TextDelta { .. } | AgentEventKind::ThinkingDelta { .. }
            )),
            "deltas must not be journaled"
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e.kind, AgentEventKind::RunStarted))
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e.kind, AgentEventKind::MessageStarted { .. }))
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e.kind, AgentEventKind::MessageCommitted { .. }))
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e.kind, AgentEventKind::RunFinished))
        );

        let seqs: Vec<u64> = events.iter().map(|e| e.seq).collect();
        assert!(seqs.windows(2).all(|w| w[0] < w[1]));
        assert_eq!(
            super::load_last_event_seq(&db, thread_id).await,
            *seqs.last().unwrap()
        );
    }

    // 5c: `?after=` replay returns exactly the journaled events past the cursor, in order — the
    // same set a reconnecting client or a `Lagged` resync recovers.
    #[tokio::test]
    async fn journal_replay_after_cursor_is_exact() {
        let db = ConnectionPool::new("sqlite::memory:").unwrap();
        db.initialize_database(db::get_migrations()).await.unwrap();

        let owner = Uuid::now_v7();
        let thread_id = Uuid::now_v7();
        seed_owner_and_thread(&db, owner, thread_id).await;

        let pool = deterministic_pool(db.clone(), single_reply_registry("pong"));
        run_to_completion(&pool, thread_id, "ping").await;
        wait_for_finished_runs(&db, thread_id, 1).await;

        let all = super::journal_events_after(&db, thread_id, 0).await;
        assert!(all.len() >= 3, "need several events to split on a cursor");
        let cursor = all[all.len() / 2].seq;

        let tail = super::journal_events_after(&db, thread_id, cursor).await;
        assert!(
            tail.iter().all(|e| e.seq > cursor),
            "replay must exclude events at or below the cursor"
        );
        let expected: Vec<u64> = all
            .iter()
            .map(|e| e.seq)
            .filter(|seq| *seq > cursor)
            .collect();
        let got: Vec<u64> = tail.iter().map(|e| e.seq).collect();
        assert_eq!(got, expected, "replay must be exact and ordered");
    }

    #[tokio::test]
    async fn journal_keeps_only_latest_tool_progress() {
        let db = ConnectionPool::new("sqlite::memory:").unwrap();
        db.initialize_database(db::get_migrations()).await.unwrap();
        let owner = Uuid::now_v7();
        let thread_id = Uuid::now_v7();
        let run_id = RunId(Uuid::now_v7());
        let agent_id = Uuid::now_v7();
        seed_owner_and_thread(&db, owner, thread_id).await;

        for (seq, value) in [(1, "first"), (2, "latest")] {
            super::journal_event(
                &db,
                &AgentEvent {
                    id: Uuid::now_v7(),
                    seq,
                    thread_id,
                    run_id: Some(run_id),
                    agent_path: vec![agent_id],
                    kind: AgentEventKind::ToolCallProgress {
                        tool_call_id: "call_1".to_owned(),
                        payload: serde_json::json!({"value": value}),
                    },
                },
            )
            .await;
        }

        let events = super::journal_events_after(&db, thread_id, 0).await;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].seq, 2);
        assert_eq!(events[0].agent_path, vec![agent_id]);
        assert!(matches!(
            &events[0].kind,
            AgentEventKind::ToolCallProgress { payload, .. }
                if payload["value"] == "latest"
        ));
    }

    // R3/5c: a consumer that fell behind past a tool call recovers the missed structural tool
    // events from the journal — the data path the WS/Telegram `Lagged` resync replays.
    #[tokio::test]
    async fn resync_recovers_tool_events_past_cursor() {
        let db = ConnectionPool::new("sqlite::memory:").unwrap();
        db.initialize_database(db::get_migrations()).await.unwrap();

        let owner = Uuid::now_v7();
        let thread_id = Uuid::now_v7();
        seed_owner_and_thread(&db, owner, thread_id).await;

        let mut models = ModelRegistry::new();
        models.add_model(
            DEFAULT_MODEL,
            ModelRegEntry {
                api: llm::Mock::new()
                    .with_stream_chunks(vec![
                        vec![tool_call_chunk("call-1", "missing_tool", r#"{"value":1}"#)],
                        vec![text_chunk("done")],
                    ])
                    .into(),
                token: "-".to_string(),
                model_name: "mock-model".to_string(),
                reasoning_effort: None,
                vision: false,
            },
        );
        let pool = deterministic_pool(db.clone(), models);
        run_to_completion(&pool, thread_id, "run tool").await;
        wait_for_finished_runs(&db, thread_id, 1).await;

        let all = super::journal_events_after(&db, thread_id, 0).await;
        // Cursor just before the first tool event, as if the consumer lagged from there.
        let tool_started = all
            .iter()
            .find(|e| matches!(e.kind, AgentEventKind::ToolCallStarted { .. }))
            .expect("tool run must journal ToolStarted");
        let cursor = tool_started.seq - 1;

        let recovered = super::journal_events_after(&db, thread_id, cursor).await;
        assert!(
            recovered
                .iter()
                .any(|e| matches!(e.kind, AgentEventKind::ToolCallStarted { .. })),
            "resync must recover ToolStarted"
        );
        assert!(
            recovered
                .iter()
                .any(|e| matches!(e.kind, AgentEventKind::ToolCallFinished { .. })),
            "resync must recover ToolFinished"
        );
    }

    #[tokio::test]
    async fn persists_subagent_transcript_registry_and_nesting() {
        use std::collections::VecDeque;
        use std::sync::Arc;
        use std::time::Instant;

        use stride_agent::{InMemoryInteractionBroker, NoopUsageObserver};

        use crate::runner::pool::{PoolHandle, ThreadRunner, WorkerInit, WorkerState};

        let db = ConnectionPool::new("sqlite::memory:").unwrap();
        db.initialize_database(db::get_migrations()).await.unwrap();

        let owner = Uuid::now_v7();
        let thread_id = Uuid::now_v7();
        seed_owner_and_thread(&db, owner, thread_id).await;

        let run_id = RunId(Uuid::now_v7());
        let init = WorkerInit {
            db: db.clone(),
            config: Arc::new(AgentConfig {
                model_registry: ModelRegistry::new(),
                max_iterations: 4,
                usage_observer: Arc::new(NoopUsageObserver),
                ..Default::default()
            }),
            server_config: test_server_config(),
            cipher: SecretCipher::new("test-secret"),
            tools: config::Tools::default(),
            mcp_tools: Vec::new(),
            vfs: None,
            telegram_bot_token: None,
            public_url: None,
            github_runtime: None,
            email_service: None,
            google_service: None,
            system_prompt: "System prompt".to_string(),
            idle_ttl: Duration::from_secs(60),
        };
        let runner = ThreadRunner {
            owner,
            agent: None,
            cancel_tx: None,
            broker: Arc::new(InMemoryInteractionBroker::default()),
            queued: VecDeque::new(),
            last_event_seq: 0,
            next_message_seq: 0,
            status: ThreadStatus::Running { run_id },
            in_progress: None,
            message_format: MessageFormat::Html,
            last_used: Instant::now(),
        };
        let mut threads = HashMap::new();
        threads.insert(thread_id, runner);
        let state = Rc::new(RefCell::new(WorkerState {
            init,
            pool: PoolHandle::for_tests(),
            threads,
        }));

        let child = Uuid::now_v7();
        let grandchild = Uuid::now_v7();
        let child_msg = Uuid::now_v7();
        let grandchild_msg = Uuid::now_v7();

        let mut persistence = TurnPersistence::default();
        let events = vec![
            // Root spawns `child`.
            (
                vec![],
                EventKind::AgentSpawned {
                    agent_id: child,
                    parent_tool_call_id: "root-call".to_string(),
                    name: "Research flights".to_string(),
                    model: "fast".to_string(),
                },
            ),
            // Child streams an assistant message (accumulated, committed once).
            (
                vec![child],
                EventKind::MessageStarted {
                    message_id: child_msg,
                    role: MessageRole::Assistant,
                },
            ),
            (
                vec![child],
                EventKind::TextDelta {
                    message_id: child_msg,
                    delta: "hello ".to_string(),
                },
            ),
            (
                vec![child],
                EventKind::TextDelta {
                    message_id: child_msg,
                    delta: "world".to_string(),
                },
            ),
            (
                vec![child],
                EventKind::MessageCommitted {
                    message_id: child_msg,
                },
            ),
            // Child runs a tool.
            (
                vec![child],
                EventKind::ToolCallStarted {
                    tool_call_id: "t1".to_string(),
                    name: "web_search".to_string(),
                    arguments: "{}".to_string(),
                },
            ),
            (
                vec![child],
                EventKind::ToolCallFinished {
                    tool_call_id: "t1".to_string(),
                    name: "web_search".to_string(),
                    result: "search results".to_string(),
                    is_error: false,
                },
            ),
            // Child spawns a grandchild (nesting: spawn event carries child path).
            (
                vec![child],
                EventKind::AgentSpawned {
                    agent_id: grandchild,
                    parent_tool_call_id: "child-call".to_string(),
                    name: "Compare prices".to_string(),
                    model: "fast".to_string(),
                },
            ),
            (
                vec![child, grandchild],
                EventKind::MessageStarted {
                    message_id: grandchild_msg,
                    role: MessageRole::Assistant,
                },
            ),
            (
                vec![child, grandchild],
                EventKind::TextDelta {
                    message_id: grandchild_msg,
                    delta: "nested answer".to_string(),
                },
            ),
            (
                vec![child, grandchild],
                EventKind::MessageCommitted {
                    message_id: grandchild_msg,
                },
            ),
            (
                vec![child, grandchild],
                EventKind::AgentFinished {
                    agent_id: grandchild,
                    result: "nested answer".to_string(),
                },
            ),
            (
                vec![child],
                EventKind::AgentFinished {
                    agent_id: child,
                    result: "final answer".to_string(),
                },
            ),
        ];

        for (agent_path, kind) in events {
            let event = ThreadEvent {
                id: Uuid::now_v7(),
                run_id: run_id.0,
                agent_path,
                kind,
            };
            process_thread_event(
                &state,
                thread_id,
                run_id,
                MessageFormat::Html,
                &mut persistence,
                event,
            )
            .await
            .unwrap();
        }

        // (a) main chat (agent_path IS NULL) has no subagent rows.
        let root_rows = db
            .query_with_params(
                "SELECT COUNT(*) AS n FROM messages WHERE parent_thread = ? AND agent_path IS NULL",
                vec![Value::Uuid(thread_id)],
            )
            .await
            .unwrap();
        assert_eq!(root_rows.rows()[0].get_int("n"), Some(0));

        // (b) registry lists both agents with name/model/finished/result.
        let agents = db
            .query_with_params(
                "SELECT agent_id, agent_path, name, model, finished, result FROM thread_agents \
                 WHERE thread_id = ? ORDER BY created_at ASC",
                vec![Value::Uuid(thread_id)],
            )
            .await
            .unwrap();
        let agents = agents.rows();
        assert_eq!(agents.len(), 2);
        assert_eq!(agents[0].get_text("name"), Some("Research flights"));
        assert_eq!(agents[0].get_text("model"), Some("fast"));
        assert_eq!(
            agents[0].get_text("agent_path"),
            Some(child.to_string().as_str())
        );
        assert_eq!(agents[0].get_text("result"), Some("final answer"));
        assert_eq!(
            agents[1].get_text("agent_path"),
            Some(format!("{child}/{grandchild}").as_str())
        );
        assert_eq!(agents[1].get_text("result"), Some("nested answer"));

        // (c) child transcript: assistant text committed once, tool call + result
        // persisted with content_format = markdown.
        let child_key = child.to_string();
        let child_rows = db
            .query_with_params(
                "SELECT role, content, content_format, tool_calls, tool_call_id FROM messages \
                 WHERE parent_thread = ? AND agent_path = ? ORDER BY seq ASC",
                vec![Value::Uuid(thread_id), Value::Text(child_key.clone())],
            )
            .await
            .unwrap();
        let child_rows = child_rows.rows();
        assert_eq!(child_rows.len(), 2);
        assert_eq!(child_rows[0].get_text("role"), Some("agent"));
        assert_eq!(child_rows[0].get_text("content"), Some("hello world"));
        assert_eq!(child_rows[0].get_text("content_format"), Some("markdown"));
        assert!(
            child_rows[0]
                .get_text("tool_calls")
                .unwrap()
                .contains("web_search")
        );
        assert_eq!(child_rows[1].get_text("role"), Some("tool"));
        assert_eq!(child_rows[1].get_text("content"), Some("search results"));
        assert_eq!(child_rows[1].get_text("tool_call_id"), Some("t1"));

        // (d) transcript endpoint prefix filter: querying the child returns its own
        // messages plus the grandchild's (agent_path = child OR child/%), proving
        // the UI-visible data is complete without any journaled deltas.
        let prefix = format!("{child}/");
        let descendants = db
            .query_with_params(
                "SELECT content FROM messages WHERE parent_thread = ? \
                 AND (agent_path = ? OR agent_path LIKE ? || '%') ORDER BY seq ASC",
                vec![
                    Value::Uuid(thread_id),
                    Value::Text(child_key),
                    Value::Text(prefix),
                ],
            )
            .await
            .unwrap();
        let contents: Vec<_> = descendants
            .rows()
            .iter()
            .filter_map(|r| r.get_text("content").map(str::to_string))
            .collect();
        assert!(contents.iter().any(|c| c == "hello world"));
        assert!(contents.iter().any(|c| c == "nested answer"));
    }
}
