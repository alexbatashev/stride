use std::{
    collections::HashMap,
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
use futures::channel::oneshot;
use stride_agent::AgentResponseChunk;

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
    pub kind: String,
    pub content: Option<String>,
    pub name: Option<String>,
    pub approval_id: Option<String>,
    pub message: Option<String>,
    pub approved: Option<bool>,
    pub error: Option<String>,
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
    pending_approvals: SharedApprovals,
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
        let Ok(mut pending) = self.pending_approvals.lock() else {
            self.event_sink
                .emit(OperatorEvent::failed("operator approval lock poisoned"));
            return false;
        };
        let Some(sender) = pending.remove(&approval_id) else {
            return false;
        };
        let sent = sender.send(approved).is_ok();
        if sent {
            self.event_sink.emit(OperatorEvent {
                kind: "approval_resolved".to_string(),
                approval_id: Some(approval_id),
                approved: Some(approved),
                ..OperatorEvent::empty()
            });
        }
        sent
    }
}

impl OperatorThreadHandle {
    fn spawn(request: OperatorThreadRequest) -> Arc<Self> {
        let (worker_tx, worker_rx) = mpsc::channel();
        let (init_tx, init_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        let event_sink = EventSink::new(event_tx);
        let pending_approvals = Arc::new(Mutex::new(HashMap::new()));

        thread::spawn({
            let event_sink = event_sink.clone();
            let pending_approvals = pending_approvals.clone();
            move || run_worker(request, worker_rx, init_tx, event_sink, pending_approvals)
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
            pending_approvals,
        })
    }
}

type SharedApprovals = Arc<Mutex<HashMap<String, oneshot::Sender<bool>>>>;

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

    fn emit(&self, mut event: OperatorEvent) {
        event.seq = self.seq.fetch_add(1, Ordering::Relaxed);
        let _ = self.sender.send(event);
    }

    fn failed(&self, error: impl Into<String>) -> OperatorEvent {
        OperatorEvent {
            seq: self.seq.fetch_add(1, Ordering::Relaxed),
            kind: "run_failed".to_string(),
            content: None,
            name: None,
            approval_id: None,
            message: None,
            approved: None,
            error: Some(error.into()),
        }
    }
}

impl OperatorEvent {
    fn empty() -> Self {
        Self {
            seq: 0,
            kind: String::new(),
            content: None,
            name: None,
            approval_id: None,
            message: None,
            approved: None,
            error: None,
        }
    }

    fn failed(error: impl Into<String>) -> Self {
        Self {
            kind: "run_failed".to_string(),
            error: Some(error.into()),
            ..Self::empty()
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
    pending_approvals: SharedApprovals,
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
                    pending_approvals.clone(),
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
    pending_approvals: SharedApprovals,
) -> OperatorTurnResult {
    let mut output = String::new();
    let mut stream = thread.make_turn(content).await;

    while let Some(chunk) = stream.next().await {
        match chunk {
            Ok(AgentResponseChunk::Chunk(chunk)) => {
                for choice in chunk.choices {
                    if let Some(delta) = choice.delta
                        && let Some(content) = delta.content
                    {
                        event_sink.emit(OperatorEvent {
                            kind: "agent_delta".to_string(),
                            content: Some(content.clone()),
                            ..OperatorEvent::empty()
                        });
                        output.push_str(&content);
                    }
                }
            }
            Ok(AgentResponseChunk::ToolStarted { name, .. }) => {
                event_sink.emit(OperatorEvent {
                    kind: "tool_started".to_string(),
                    name: Some(name.clone()),
                    ..OperatorEvent::empty()
                });
                if output.is_empty() {
                    output.push_str(&format!("Running {name}..."));
                }
            }
            Ok(AgentResponseChunk::ToolFinished { name, .. }) => {
                event_sink.emit(OperatorEvent {
                    kind: "tool_finished".to_string(),
                    name: Some(name),
                    ..OperatorEvent::empty()
                });
            }
            Ok(AgentResponseChunk::Approval {
                tool_name,
                message,
                approved,
            }) => {
                let approval_id = format!("approval-{}", event_sink.seq.load(Ordering::Relaxed));
                let Ok(mut pending) = pending_approvals.lock() else {
                    return OperatorTurnResult::failed("operator approval lock poisoned");
                };
                pending.insert(approval_id.clone(), approved);
                drop(pending);
                event_sink.emit(OperatorEvent {
                    kind: "waiting_for_approval".to_string(),
                    name: Some(tool_name),
                    approval_id: Some(approval_id),
                    message: Some(message),
                    ..OperatorEvent::empty()
                });
            }
            Ok(AgentResponseChunk::Quiz { tool_name, .. }) => {
                return OperatorTurnResult::failed(format!("{tool_name} requires input"));
            }
            Err(error) => return OperatorTurnResult::failed(error.to_string()),
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
}
