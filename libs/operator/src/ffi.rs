use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

use crate::{Operator, OperatorConfig};
use futures::StreamExt;
use stride_agent::{
    EventKind, InMemoryInteractionBroker, InteractionBroker, ThreadEvent, TurnContext,
};
use uuid::Uuid;

#[derive(Clone, Debug, uniffi::Record)]
pub struct OperatorThreadSummary {
    pub id: String,
    pub title: String,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct OperatorTurnResult {
    pub content: String,
    pub error: Option<String>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct OperatorEvent {
    pub seq: u64,
    pub event_id: String,
    pub run_id: String,
    pub agent_path: Vec<String>,
    pub kind: OperatorEventKind,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct OperatorQuizQuestion {
    pub question: String,
    pub options: Vec<String>,
}

#[derive(Clone, Debug, uniffi::Enum)]
pub enum OperatorEventKind {
    RunStarted,
    RunFinished,
    RunFailed {
        error: String,
    },
    RunCancelled,
    MessageStarted {
        message_id: String,
        role: String,
    },
    TextDelta {
        message_id: String,
        delta: String,
    },
    ThinkingDelta {
        message_id: String,
        delta: String,
    },
    MessageCommitted {
        message_id: String,
    },
    ToolCallStarted {
        tool_call_id: String,
        name: String,
        arguments: String,
    },
    ToolCallProgress {
        tool_call_id: String,
        payload: String,
    },
    ToolCallFinished {
        tool_call_id: String,
        name: String,
        result: String,
        is_error: bool,
    },
    AgentSpawned {
        agent_id: String,
        parent_tool_call_id: String,
        name: String,
        model: String,
    },
    AgentFinished {
        agent_id: String,
        result: String,
    },
    ApprovalRequested {
        approval_id: String,
        tool_call_id: String,
        message: String,
    },
    ApprovalResolved {
        approval_id: String,
        approved: bool,
    },
    QuizRequested {
        quiz_id: String,
        questions: Vec<OperatorQuizQuestion>,
    },
    QuizAnswered {
        quiz_id: String,
    },
}

#[derive(uniffi::Object)]
pub struct OperatorRuntime {
    cloud_base_url: String,
    bearer_token: String,
    model: String,
    working_directory: Option<String>,
}

#[uniffi::export]
impl OperatorRuntime {
    #[uniffi::constructor]
    pub fn new(
        cloud_base_url: String,
        bearer_token: String,
        model: Option<String>,
        working_directory: Option<String>,
    ) -> Arc<Self> {
        Arc::new(Self {
            cloud_base_url,
            bearer_token,
            model: model.unwrap_or_else(|| "default".to_string()),
            working_directory,
        })
    }

    pub fn new_thread(&self) -> Arc<OperatorThreadHandle> {
        OperatorThreadHandle::spawn(OperatorThreadRequest {
            cloud_base_url: self.cloud_base_url.clone(),
            bearer_token: self.bearer_token.clone(),
            model: self.model.clone(),
            working_directory: self.working_directory.clone(),
        })
    }
}

#[derive(uniffi::Object)]
pub struct OperatorThreadHandle {
    summary: OperatorThreadSummary,
    tool_names: Vec<String>,
    sender: Mutex<mpsc::Sender<WorkerRequest>>,
    event_receiver: Mutex<mpsc::Receiver<OperatorEvent>>,
    event_sink: EventSink,
    broker: Arc<InMemoryInteractionBroker>,
}

#[uniffi::export]
impl OperatorThreadHandle {
    pub fn summary(&self) -> OperatorThreadSummary {
        self.summary.clone()
    }

    pub fn tool_names(&self) -> Vec<String> {
        self.tool_names.clone()
    }

    pub fn send_message(&self, content: String) -> OperatorTurnResult {
        let (response_tx, response_rx) = mpsc::channel();
        let Ok(sender) = self.sender.lock() else {
            return OperatorTurnResult::failed("operator worker lock poisoned");
        };
        if sender
            .send(WorkerRequest::SendMessage {
                content,
                response: response_tx,
            })
            .is_err()
        {
            return OperatorTurnResult::failed("operator worker stopped");
        }
        response_rx
            .recv()
            .unwrap_or_else(|_| OperatorTurnResult::failed("operator worker stopped"))
    }

    pub fn next_event(&self, timeout_ms: u64) -> Option<OperatorEvent> {
        let Ok(receiver) = self.event_receiver.lock() else {
            return Some(
                self.event_sink
                    .failed("operator event receiver lock poisoned"),
            );
        };
        receiver
            .recv_timeout(Duration::from_millis(timeout_ms))
            .ok()
    }

    pub fn resolve_approval(&self, approval_id: String, approved: bool) -> bool {
        Uuid::parse_str(&approval_id)
            .ok()
            .is_some_and(|id| self.broker.resolve_approval(id, approved))
    }

    pub fn answer_quiz(&self, quiz_id: String, answers: Vec<String>) -> bool {
        Uuid::parse_str(&quiz_id)
            .ok()
            .is_some_and(|id| self.broker.answer_quiz(id, answers))
    }
}

impl OperatorThreadHandle {
    fn spawn(request: OperatorThreadRequest) -> Arc<Self> {
        let (worker_tx, worker_rx) = mpsc::channel();
        let (init_tx, init_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        let event_sink = EventSink::new(event_tx);
        let broker = Arc::new(InMemoryInteractionBroker::default());

        thread::spawn({
            let event_sink = event_sink.clone();
            let broker = broker.clone();
            move || run_worker(request, worker_rx, init_tx, event_sink, broker)
        });

        let init = init_rx
            .recv()
            .unwrap_or_else(|error| WorkerInit::failed(format!("operator worker failed: {error}")));

        Arc::new(Self {
            summary: init.summary,
            tool_names: init.tool_names,
            sender: Mutex::new(worker_tx),
            event_receiver: Mutex::new(event_rx),
            event_sink,
            broker,
        })
    }
}

#[derive(Clone)]
struct EventSink {
    sender: mpsc::Sender<OperatorEvent>,
    seq: Arc<AtomicU64>,
}

impl EventSink {
    fn new(sender: mpsc::Sender<OperatorEvent>) -> Self {
        Self {
            sender,
            seq: Arc::new(AtomicU64::new(1)),
        }
    }

    fn send(&self, mut event: OperatorEvent) {
        event.seq = self.seq.fetch_add(1, Ordering::Relaxed);
        let _ = self.sender.send(event);
    }

    fn failed(&self, error: impl Into<String>) -> OperatorEvent {
        OperatorEvent {
            seq: self.seq.fetch_add(1, Ordering::Relaxed),
            event_id: String::new(),
            run_id: String::new(),
            agent_path: Vec::new(),
            kind: OperatorEventKind::RunFailed {
                error: error.into(),
            },
        }
    }
}

impl stride_agent::EventSink for EventSink {
    fn emit(&self, event: ThreadEvent) {
        self.send(event.into());
    }
}

impl From<ThreadEvent> for OperatorEvent {
    fn from(event: ThreadEvent) -> Self {
        Self {
            seq: 0,
            event_id: event.id.to_string(),
            run_id: event.run_id.to_string(),
            agent_path: event
                .agent_path
                .into_iter()
                .map(|id| id.to_string())
                .collect(),
            kind: match event.kind {
                EventKind::RunStarted => OperatorEventKind::RunStarted,
                EventKind::RunFinished => OperatorEventKind::RunFinished,
                EventKind::RunFailed { error } => OperatorEventKind::RunFailed { error },
                EventKind::RunCancelled => OperatorEventKind::RunCancelled,
                EventKind::MessageStarted { message_id, role } => {
                    OperatorEventKind::MessageStarted {
                        message_id: message_id.to_string(),
                        role: format!("{role:?}").to_lowercase(),
                    }
                }
                EventKind::TextDelta { message_id, delta } => OperatorEventKind::TextDelta {
                    message_id: message_id.to_string(),
                    delta,
                },
                EventKind::ThinkingDelta { message_id, delta } => {
                    OperatorEventKind::ThinkingDelta {
                        message_id: message_id.to_string(),
                        delta,
                    }
                }
                EventKind::MessageCommitted { message_id } => OperatorEventKind::MessageCommitted {
                    message_id: message_id.to_string(),
                },
                EventKind::ToolCallStarted {
                    tool_call_id,
                    name,
                    arguments,
                } => OperatorEventKind::ToolCallStarted {
                    tool_call_id,
                    name,
                    arguments,
                },
                EventKind::ToolCallProgress {
                    tool_call_id,
                    payload,
                } => OperatorEventKind::ToolCallProgress {
                    tool_call_id,
                    payload: payload.to_string(),
                },
                EventKind::ToolCallFinished {
                    tool_call_id,
                    name,
                    result,
                    is_error,
                } => OperatorEventKind::ToolCallFinished {
                    tool_call_id,
                    name,
                    result,
                    is_error,
                },
                EventKind::AgentSpawned {
                    agent_id,
                    parent_tool_call_id,
                    name,
                    model,
                } => OperatorEventKind::AgentSpawned {
                    agent_id: agent_id.to_string(),
                    parent_tool_call_id,
                    name,
                    model,
                },
                EventKind::AgentFinished { agent_id, result } => OperatorEventKind::AgentFinished {
                    agent_id: agent_id.to_string(),
                    result,
                },
                EventKind::ApprovalRequested {
                    approval_id,
                    tool_call_id,
                    message,
                } => OperatorEventKind::ApprovalRequested {
                    approval_id: approval_id.to_string(),
                    tool_call_id,
                    message,
                },
                EventKind::ApprovalResolved {
                    approval_id,
                    approved,
                } => OperatorEventKind::ApprovalResolved {
                    approval_id: approval_id.to_string(),
                    approved,
                },
                EventKind::QuizRequested { quiz_id, questions } => {
                    OperatorEventKind::QuizRequested {
                        quiz_id: quiz_id.to_string(),
                        questions: questions
                            .into_iter()
                            .map(|question| OperatorQuizQuestion {
                                question: question.question,
                                options: question.options,
                            })
                            .collect(),
                    }
                }
                EventKind::QuizAnswered { quiz_id } => OperatorEventKind::QuizAnswered {
                    quiz_id: quiz_id.to_string(),
                },
            },
        }
    }
}

struct OperatorThreadRequest {
    cloud_base_url: String,
    bearer_token: String,
    model: String,
    working_directory: Option<String>,
}

struct WorkerInit {
    summary: OperatorThreadSummary,
    tool_names: Vec<String>,
}

impl WorkerInit {
    fn failed(error: String) -> Self {
        Self {
            summary: OperatorThreadSummary {
                id: "local:unavailable".to_string(),
                title: error,
            },
            tool_names: Vec::new(),
        }
    }
}

enum WorkerRequest {
    SendMessage {
        content: String,
        response: mpsc::Sender<OperatorTurnResult>,
    },
}

fn run_worker(
    request: OperatorThreadRequest,
    receiver: mpsc::Receiver<WorkerRequest>,
    init: mpsc::Sender<WorkerInit>,
    event_sink: EventSink,
    broker: Arc<InMemoryInteractionBroker>,
) {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            let _ = init.send(WorkerInit::failed(format!(
                "operator runtime failed: {error}"
            )));
            return;
        }
    };

    let mut config =
        OperatorConfig::authorized_endpoint(request.cloud_base_url, request.bearer_token)
            .model(request.model);
    if let Some(working_directory) = request.working_directory {
        config = config.working_directory(working_directory);
    }

    let operator = Operator::new(config);
    let thread = operator.new_thread();
    let summary = thread.summary();
    let tool_names = thread.tool_names();
    let _ = init.send(WorkerInit {
        summary: OperatorThreadSummary {
            id: summary.id,
            title: summary.title,
        },
        tool_names,
    });

    while let Ok(request) = receiver.recv() {
        match request {
            WorkerRequest::SendMessage { content, response } => {
                let result = runtime.block_on(send_message(
                    &thread,
                    content,
                    event_sink.clone(),
                    broker.clone(),
                ));
                let _ = response.send(result);
            }
        }
    }
}

async fn send_message(
    thread: &crate::OperatorThread,
    content: String,
    event_sink: EventSink,
    broker: Arc<InMemoryInteractionBroker>,
) -> OperatorTurnResult {
    let mut output = String::new();
    let context = TurnContext::new(Uuid::now_v7(), Arc::new(event_sink), broker);
    let mut stream = thread.make_turn(content, context).await;

    while let Some(event) = stream.next().await {
        match event.kind {
            EventKind::TextDelta { delta, .. } => output.push_str(&delta),
            EventKind::ToolCallStarted { name, .. } if output.is_empty() => {
                output.push_str(&format!("Running {name}..."));
            }
            EventKind::RunFailed { error } => return OperatorTurnResult::failed(error),
            _ => {}
        }
    }

    OperatorTurnResult {
        content: output,
        error: None,
    }
}

impl OperatorTurnResult {
    fn failed(error: impl Into<String>) -> Self {
        Self {
            content: String::new(),
            error: Some(error.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thread_handle_exposes_agent_shell_tool() {
        let runtime = OperatorRuntime::new(
            "http://127.0.0.1:3000".to_string(),
            "token".to_string(),
            None,
            None,
        );
        let thread = runtime.new_thread();

        assert!(thread.summary().id.starts_with("local:"));
        assert!(thread.tool_names().contains(&"shell".to_string()));
    }

    #[test]
    fn shared_event_conversion_keeps_attachment_ids() {
        let event_id = Uuid::now_v7();
        let run_id = Uuid::now_v7();
        let message_id = Uuid::now_v7();
        let agent_id = Uuid::now_v7();
        let event = OperatorEvent::from(ThreadEvent {
            id: event_id,
            run_id,
            agent_path: vec![agent_id],
            kind: EventKind::TextDelta {
                message_id,
                delta: "hello".to_owned(),
            },
        });

        assert_eq!(event.event_id, event_id.to_string());
        assert_eq!(event.run_id, run_id.to_string());
        assert_eq!(event.agent_path, vec![agent_id.to_string()]);
        assert!(matches!(
            event.kind,
            OperatorEventKind::TextDelta {
                message_id: converted_message_id,
                delta,
            } if converted_message_id == message_id.to_string() && delta == "hello"
        ));
    }
}
