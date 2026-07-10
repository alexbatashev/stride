use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

use futures::StreamExt;
use stride_agent::{
    AgentResponseChunk,
    sanitizer::{HtmlFormattingSanitizer, StreamingMessageSanitizer},
};
use tokio::sync::watch;
use uuid::Uuid;

use crate::{
    db::{MessageFormat, Role, messages, threads},
    model_registry,
    runner::{
        AgentEventKind, AgentPoolError, PartialAgentMessage, RunId, db_error,
        pool::{PendingApprovalState, PendingQuizState, WorkerState, drain_queue, with_runner},
        thread_events_topic,
    },
};

struct AssistantMessageState {
    id: Option<Uuid>,
    seq: Option<u64>,
    content: String,
    thinking: Option<String>,
    tool_calls: BTreeMap<usize, PartialToolCall>,
    format: MessageFormat,
    output_sanitizer: Box<dyn StreamingMessageSanitizer>,
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
    let mut stream = agent
        .make_turn(with_format_reminder(content, format), images)
        .await;
    let mut assistant = AssistantMessageState {
        id: None,
        seq: None,
        content: String::new(),
        thinking: None,
        tool_calls: BTreeMap::new(),
        format,
        output_sanitizer: output_sanitizer(format, &state),
    };

    loop {
        tokio::select! {
            biased;
            _ = cancel_rx.changed() => {
                cancel_run_task(&state, thread_id, run_id).await;
                restore_agent(&state, thread_id, agent);
                drain_queue(&state, thread_id).await;
                return;
            }
            item = stream.next() => {
                let Some(item) = item else { break; };
                match item {
                    Ok(AgentResponseChunk::Chunk(chunk)) => {
                        if let Err(error) =
                            handle_agent_chunk(&state, thread_id, run_id, &mut assistant, chunk).await
                        {
                            fail_run(&state, thread_id, run_id, error.to_string()).await;
                            restore_agent(&state, thread_id, agent);
                            drain_queue(&state, thread_id).await;
                            return;
                        }
                    }
                    Ok(AgentResponseChunk::ToolStarted { name, .. }) => {
                        emit(
                            &state,
                            thread_id,
                            Some(run_id),
                            AgentEventKind::ToolStarted { name },
                        )
                        .await;
                    }
                    Ok(AgentResponseChunk::ToolFinished {
                        tool_call_id,
                        name,
                        result,
                    }) => {
                        if let Err(error) =
                            persist_tool_message(&state, thread_id, &tool_call_id, &result).await
                        {
                            fail_run(&state, thread_id, run_id, error.to_string()).await;
                            restore_agent(&state, thread_id, agent);
                            drain_queue(&state, thread_id).await;
                            return;
                        }

                        emit(
                            &state,
                            thread_id,
                            Some(run_id),
                            AgentEventKind::ToolFinished { name },
                        )
                        .await;
                    }
                    Ok(AgentResponseChunk::Approval {
                        message, approved, ..
                    }) => {
                        let approval_id = state.borrow().init.config.id_gen.new_uuid_v7();
                        tracing::info!(
                            %thread_id,
                            run_id = %run_id.0,
                            %approval_id,
                            "agent waiting for approval"
                        );
                        with_runner(&state, thread_id, |runner| {
                            runner.pending_approvals.insert(
                                approval_id,
                                PendingApprovalState {
                                    run_id,
                                    message: message.clone(),
                                    approved,
                                },
                            );
                        });
                        emit(
                            &state,
                            thread_id,
                            Some(run_id),
                            AgentEventKind::WaitingForApproval {
                                approval_id,
                                message,
                            },
                        )
                        .await;
                    }
                    Ok(AgentResponseChunk::Quiz {
                        questions,
                        answered,
                        ..
                    }) => {
                        tracing::info!(
                            %thread_id,
                            run_id = %run_id.0,
                            question_count = questions.len(),
                            "agent waiting for quiz answers"
                        );
                        // An empty question set has nothing to present; resolve it here so the
                        // agent never blocks on a dispatcher that cannot answer it.
                        if questions.is_empty() {
                            let _ = answered.send(Vec::new());
                            continue;
                        }
                        let quiz_id = state.borrow().init.config.id_gen.new_uuid_v7();
                        with_runner(&state, thread_id, |runner| {
                            runner.pending_quizzes.insert(
                                quiz_id,
                                PendingQuizState {
                                    run_id,
                                    questions: questions.clone(),
                                    answered,
                                },
                            );
                        });
                        emit(
                            &state,
                            thread_id,
                            Some(run_id),
                            AgentEventKind::WaitingForQuiz { quiz_id, questions },
                        )
                        .await;
                    }
                    Err(error) => {
                        fail_run(&state, thread_id, run_id, error.to_string()).await;
                        restore_agent(&state, thread_id, agent);
                        drain_queue(&state, thread_id).await;
                        return;
                    }
                }
            }
        }
    }

    let clock = state.borrow().init.config.clock.clone();
    with_runner(&state, thread_id, |runner| {
        runner.cancel_tx = None;
        runner.pending_approvals.clear();
        runner.pending_quizzes.clear();
        runner.status = crate::runner::ThreadStatus::Idle;
        runner.in_progress = None;
        runner.last_used = clock.now_instant();
    });
    emit(&state, thread_id, Some(run_id), AgentEventKind::RunFinished).await;
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

async fn handle_agent_chunk(
    state: &Rc<RefCell<WorkerState>>,
    thread_id: Uuid,
    run_id: RunId,
    assistant: &mut AssistantMessageState,
    chunk: llm::StreamResponseChunk,
) -> Result<(), AgentPoolError> {
    let mut has_message_delta = false;

    for choice in &chunk.choices {
        if let Some(message) = &choice.message {
            if !message.content.is_empty() {
                has_message_delta = true;
                append_assistant_content(state, thread_id, run_id, assistant, &message.content)
                    .await?;
            }

            if let Some(thinking) = message
                .thinking
                .as_ref()
                .filter(|thinking| !thinking.is_empty())
            {
                ensure_assistant_message(state, thread_id, assistant).await?;
                has_message_delta = true;
                assistant
                    .thinking
                    .get_or_insert_with(String::new)
                    .push_str(thinking);
                emit(
                    state,
                    thread_id,
                    Some(run_id),
                    AgentEventKind::ThinkingDelta {
                        thinking: thinking.clone(),
                    },
                )
                .await;
            }

            if let Some(chunks) = message
                .tool_calls
                .as_ref()
                .filter(|chunks| has_tool_call_data(chunks))
            {
                ensure_assistant_message(state, thread_id, assistant).await?;
                has_message_delta = true;
                append_tool_call_chunks(&mut assistant.tool_calls, chunks);
            }
        }

        if let Some(content) = choice.text.as_ref().filter(|content| !content.is_empty()) {
            has_message_delta = true;
            append_assistant_content(state, thread_id, run_id, assistant, content).await?;
        }

        if let Some(delta) = &choice.delta {
            if let Some(content) = delta.content.as_ref().filter(|content| !content.is_empty()) {
                has_message_delta = true;
                append_assistant_content(state, thread_id, run_id, assistant, content).await?;
            }

            if let Some(thinking) = delta
                .thinking
                .as_ref()
                .filter(|thinking| !thinking.is_empty())
            {
                ensure_assistant_message(state, thread_id, assistant).await?;
                has_message_delta = true;
                assistant
                    .thinking
                    .get_or_insert_with(String::new)
                    .push_str(thinking);
                emit(
                    state,
                    thread_id,
                    Some(run_id),
                    AgentEventKind::ThinkingDelta {
                        thinking: thinking.clone(),
                    },
                )
                .await;
            }

            if let Some(chunks) = delta
                .tool_calls
                .as_ref()
                .filter(|chunks| has_tool_call_data(chunks))
            {
                ensure_assistant_message(state, thread_id, assistant).await?;
                has_message_delta = true;
                append_tool_call_chunks(&mut assistant.tool_calls, chunks);
            }
        }
    }

    if has_message_delta && let Some(id) = assistant.id {
        let db = state.borrow().init.db.clone();
        update_message(
            &db,
            id,
            &assistant.content,
            assistant.thinking.as_deref(),
            None,
        )
        .await?;

        with_runner(state, thread_id, |runner| {
            runner.in_progress = Some(PartialAgentMessage {
                run_id,
                content: assistant.content.clone(),
                thinking: assistant.thinking.clone(),
                format: assistant.format,
            });
        });
    }

    if chunk
        .choices
        .iter()
        .any(|choice| choice.finish_reason.is_some())
    {
        if let (Some(message_id), Some(seq)) = (assistant.id, assistant.seq) {
            assistant.content = assistant.output_sanitizer.finish();
            let tool_calls = serialize_tool_calls(&assistant.tool_calls)?;
            let db = state.borrow().init.db.clone();
            update_message(
                &db,
                message_id,
                &assistant.content,
                assistant.thinking.as_deref(),
                tool_calls.as_deref(),
            )
            .await?;

            emit(
                state,
                thread_id,
                Some(run_id),
                AgentEventKind::AgentMessageCommitted { message_id, seq },
            )
            .await;
        }

        assistant.id = None;
        assistant.seq = None;
        assistant.content.clear();
        assistant.thinking = None;
        assistant.tool_calls.clear();
        assistant.output_sanitizer = output_sanitizer(assistant.format, state);
    }

    Ok(())
}

async fn append_assistant_content(
    state: &Rc<RefCell<WorkerState>>,
    thread_id: Uuid,
    run_id: RunId,
    assistant: &mut AssistantMessageState,
    content: &str,
) -> Result<(), AgentPoolError> {
    ensure_assistant_message(state, thread_id, assistant).await?;
    assistant.output_sanitizer.push_str(content);
    assistant.content = assistant.output_sanitizer.snapshot();
    emit(
        state,
        thread_id,
        Some(run_id),
        AgentEventKind::AgentDelta {
            content: assistant.content.clone(),
            format: assistant.format,
        },
    )
    .await;
    Ok(())
}

fn has_tool_call_data(chunks: &[llm::ToolCallChunk]) -> bool {
    chunks.iter().any(|chunk| {
        chunk.id.as_ref().is_some_and(|id| !id.is_empty())
            || chunk.function.as_ref().is_some_and(|function| {
                function.name.as_ref().is_some_and(|name| !name.is_empty())
                    || function
                        .arguments
                        .as_ref()
                        .is_some_and(|arguments| !arguments.is_empty())
            })
    })
}

fn append_tool_call_chunks(
    tool_calls: &mut BTreeMap<usize, PartialToolCall>,
    chunks: &[llm::ToolCallChunk],
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

fn serialize_tool_calls(
    tool_calls: &BTreeMap<usize, PartialToolCall>,
) -> Result<Option<String>, AgentPoolError> {
    let calls: Vec<_> = tool_calls
        .values()
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

async fn ensure_assistant_message(
    state: &Rc<RefCell<WorkerState>>,
    thread_id: Uuid,
    assistant: &mut AssistantMessageState,
) -> Result<(), AgentPoolError> {
    if assistant.id.is_some() {
        return Ok(());
    }

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
        .role(Role::Agent)
        .content("")
        .content_format(assistant.format)
        .images(Option::<&str>::None)
        .thinking(Option::<&str>::None)
        .tool_calls(Option::<&str>::None)
        .tool_call_id(Option::<&str>::None)
        .execute(&db)
        .await
        .map_err(db_error)?;

    assistant.id = Some(id);
    assistant.seq = Some(seq);
    assistant.content.clear();
    assistant.thinking = None;
    assistant.tool_calls.clear();
    assistant.output_sanitizer = output_sanitizer(assistant.format, state);

    Ok(())
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
        runner.pending_approvals.clear();
        runner.pending_quizzes.clear();
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
        runner.pending_approvals.clear();
        runner.pending_quizzes.clear();
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
    let event = {
        let mut state = state.borrow_mut();
        let Some(runner) = state.threads.get_mut(&thread_id) else {
            return;
        };
        runner.last_event_seq += 1;
        crate::runner::AgentEvent {
            seq: runner.last_event_seq,
            thread_id,
            run_id,
            kind,
        }
    };

    let _ =
        pubsub::topic::<crate::runner::AgentEvent>(&thread_events_topic(thread_id)).publish(&event);
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
                observer: std::sync::Arc::new(stride_agent::NoopAgentObserver),
                ..Default::default()
            }),
            test_server_config(),
            SecretCipher::new("test-secret"),
        )
        .system_prompt("System prompt")
        .idle_ttl(Duration::from_secs(60))
        .build()
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
                AgentEventKind::AgentDelta { content, .. } if content == "pong" => {
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
                AgentEventKind::ToolStarted { name } if name == "missing_tool" => {
                    saw_tool_started = true;
                }
                AgentEventKind::ToolFinished { name } if name == "missing_tool" => {
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
                AgentEventKind::AgentDelta { content, .. }
                    if content == "<h1>Helloalert(1)</h1>" =>
                {
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
            if let AgentEventKind::WaitingForQuiz { quiz_id: id, .. } = event.kind {
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
                AgentEventKind::AgentDelta { .. } => saw_delta = true,
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
}
