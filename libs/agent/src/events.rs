use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use futures::channel::oneshot;
use futures::future::{FutureExt, LocalBoxFuture};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::IdGen;
use crate::QuizQuestion;

pub type EventId = Uuid;
pub type RunId = Uuid;
pub type AgentId = Uuid;
pub type MessageId = Uuid;
pub type ApprovalId = Uuid;
pub type QuizId = Uuid;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ThreadEvent {
    pub id: EventId,
    pub run_id: RunId,
    pub agent_path: Vec<AgentId>,
    pub kind: EventKind,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventKind {
    RunStarted,
    RunFinished,
    RunFailed {
        error: String,
    },
    RunCancelled,
    MessageStarted {
        message_id: MessageId,
        role: MessageRole,
    },
    TextDelta {
        message_id: MessageId,
        delta: String,
    },
    ThinkingDelta {
        message_id: MessageId,
        delta: String,
    },
    MessageCommitted {
        message_id: MessageId,
    },
    ToolCallStarted {
        tool_call_id: String,
        name: String,
        arguments: String,
    },
    ToolCallProgress {
        tool_call_id: String,
        payload: Value,
    },
    ToolCallFinished {
        tool_call_id: String,
        name: String,
        result: String,
        is_error: bool,
    },
    AgentSpawned {
        agent_id: AgentId,
        parent_tool_call_id: String,
        name: String,
        model: String,
    },
    AgentFinished {
        agent_id: AgentId,
        result: String,
    },
    ApprovalRequested {
        approval_id: ApprovalId,
        tool_call_id: String,
        message: String,
    },
    ApprovalResolved {
        approval_id: ApprovalId,
        approved: bool,
    },
    QuizRequested {
        quiz_id: QuizId,
        questions: Vec<QuizQuestion>,
    },
    QuizAnswered {
        quiz_id: QuizId,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    User,
    Assistant,
    System,
    Tool,
}

pub trait EventSink: Send + Sync {
    fn emit(&self, event: ThreadEvent);
}

#[derive(Clone)]
pub struct TurnContext {
    pub run_id: RunId,
    pub agent_path: Vec<AgentId>,
    pub sink: Arc<dyn EventSink>,
    pub broker: Arc<dyn InteractionBroker>,
    pub emit_user_message_events: bool,
}

impl TurnContext {
    pub fn new(
        run_id: RunId,
        sink: Arc<dyn EventSink>,
        broker: Arc<dyn InteractionBroker>,
    ) -> Self {
        Self {
            run_id,
            agent_path: Vec::new(),
            sink,
            broker,
            emit_user_message_events: true,
        }
    }

    pub fn without_user_message_events(mut self) -> Self {
        self.emit_user_message_events = false;
        self
    }

    pub fn child(&self, agent_id: AgentId) -> Self {
        let mut child = self.clone();
        child.agent_path.push(agent_id);
        child
    }

    pub fn with_broker(mut self, broker: Arc<dyn InteractionBroker>) -> Self {
        self.broker = broker;
        self
    }

    pub(crate) fn with_sink(mut self, sink: Arc<dyn EventSink>) -> Self {
        self.sink = sink;
        self
    }

    pub(crate) fn emit(&self, id_gen: &dyn IdGen, kind: EventKind) {
        self.sink.emit(ThreadEvent {
            id: id_gen.new_uuid_v7(),
            run_id: self.run_id,
            agent_path: self.agent_path.clone(),
            kind,
        });
    }
}

#[derive(Clone)]
pub struct ToolContext {
    pub turn: TurnContext,
    pub tool_call_id: String,
    id_gen: Arc<dyn IdGen>,
}

impl ToolContext {
    pub(crate) fn new(turn: TurnContext, tool_call_id: String, id_gen: Arc<dyn IdGen>) -> Self {
        Self {
            turn,
            tool_call_id,
            id_gen,
        }
    }

    pub fn emit(&self, kind: EventKind) {
        self.turn.emit(self.id_gen.as_ref(), kind);
    }

    pub fn progress(&self, payload: Value) {
        self.emit(EventKind::ToolCallProgress {
            tool_call_id: self.tool_call_id.clone(),
            payload,
        });
    }

    pub fn child_turn(&self, agent_id: AgentId) -> TurnContext {
        self.turn.child(agent_id)
    }

    pub async fn request_approval(&self, message: String) -> bool {
        let approval_id = self.id_gen.new_uuid_v7();
        let response = self.turn.broker.request_approval(
            approval_id,
            self.tool_call_id.clone(),
            message.clone(),
        );
        self.emit(EventKind::ApprovalRequested {
            approval_id,
            tool_call_id: self.tool_call_id.clone(),
            message,
        });
        let approved = response.await;
        self.emit(EventKind::ApprovalResolved {
            approval_id,
            approved,
        });
        approved
    }

    pub async fn request_quiz(&self, questions: Vec<QuizQuestion>) -> Vec<String> {
        if questions.is_empty() {
            return Vec::new();
        }
        let quiz_id = self.id_gen.new_uuid_v7();
        let response = self.turn.broker.request_quiz(quiz_id, questions.clone());
        self.emit(EventKind::QuizRequested { quiz_id, questions });
        let answers = response.await;
        self.emit(EventKind::QuizAnswered { quiz_id });
        answers
    }
}

#[derive(Debug, Default)]
pub struct NoopEventSink;

impl EventSink for NoopEventSink {
    fn emit(&self, _event: ThreadEvent) {}
}

pub trait InteractionBroker: Send + Sync {
    fn request_approval(
        &self,
        id: ApprovalId,
        tool_call_id: String,
        message: String,
    ) -> LocalBoxFuture<'static, bool>;
    fn request_quiz(
        &self,
        id: QuizId,
        questions: Vec<QuizQuestion>,
    ) -> LocalBoxFuture<'static, Vec<String>>;
    fn resolve_approval(&self, id: ApprovalId, approved: bool) -> bool;
    fn answer_quiz(&self, id: QuizId, answers: Vec<String>) -> bool;
}

#[derive(Debug, Default)]
pub struct AutoDenyInteractionBroker;

impl InteractionBroker for AutoDenyInteractionBroker {
    fn request_approval(
        &self,
        _id: ApprovalId,
        _tool_call_id: String,
        _message: String,
    ) -> LocalBoxFuture<'static, bool> {
        async { false }.boxed_local()
    }

    fn request_quiz(
        &self,
        _id: QuizId,
        _questions: Vec<QuizQuestion>,
    ) -> LocalBoxFuture<'static, Vec<String>> {
        async { Vec::new() }.boxed_local()
    }

    fn resolve_approval(&self, _id: ApprovalId, _approved: bool) -> bool {
        false
    }

    fn answer_quiz(&self, _id: QuizId, _answers: Vec<String>) -> bool {
        false
    }
}

#[derive(Clone, Default)]
pub struct InMemoryInteractionBroker {
    inner: Arc<BrokerState>,
}

#[derive(Default)]
struct BrokerState {
    approvals: Mutex<HashMap<ApprovalId, PendingApprovalEntry>>,
    quizzes: Mutex<Vec<PendingQuizEntry>>,
}

struct PendingApprovalEntry {
    tool_call_id: String,
    message: String,
    sender: oneshot::Sender<bool>,
}

struct PendingQuizEntry {
    id: QuizId,
    questions: Vec<QuizQuestion>,
    sender: oneshot::Sender<Vec<String>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingApprovalInteraction {
    pub id: ApprovalId,
    pub tool_call_id: String,
    pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingQuizInteraction {
    pub id: QuizId,
    pub questions: Vec<QuizQuestion>,
}

impl InMemoryInteractionBroker {
    pub fn pending_approvals(&self) -> Vec<PendingApprovalInteraction> {
        self.inner
            .approvals
            .lock()
            .unwrap()
            .iter()
            .map(|(id, entry)| PendingApprovalInteraction {
                id: *id,
                tool_call_id: entry.tool_call_id.clone(),
                message: entry.message.clone(),
            })
            .collect()
    }

    pub fn pending_quizzes(&self) -> Vec<PendingQuizInteraction> {
        self.inner
            .quizzes
            .lock()
            .unwrap()
            .iter()
            .map(|entry| PendingQuizInteraction {
                id: entry.id,
                questions: entry.questions.clone(),
            })
            .collect()
    }

    pub fn clear(&self) {
        self.inner.approvals.lock().unwrap().clear();
        self.inner.quizzes.lock().unwrap().clear();
    }
}

impl InteractionBroker for InMemoryInteractionBroker {
    fn request_approval(
        &self,
        id: ApprovalId,
        tool_call_id: String,
        message: String,
    ) -> LocalBoxFuture<'static, bool> {
        let (sender, receiver) = oneshot::channel();
        self.inner.approvals.lock().unwrap().insert(
            id,
            PendingApprovalEntry {
                tool_call_id,
                message,
                sender,
            },
        );
        async move { receiver.await.unwrap_or(false) }.boxed_local()
    }

    fn request_quiz(
        &self,
        id: QuizId,
        questions: Vec<QuizQuestion>,
    ) -> LocalBoxFuture<'static, Vec<String>> {
        let (sender, receiver) = oneshot::channel();
        self.inner.quizzes.lock().unwrap().push(PendingQuizEntry {
            id,
            questions,
            sender,
        });
        async move { receiver.await.unwrap_or_default() }.boxed_local()
    }

    fn resolve_approval(&self, id: ApprovalId, approved: bool) -> bool {
        self.inner
            .approvals
            .lock()
            .unwrap()
            .remove(&id)
            .is_some_and(|entry| entry.sender.send(approved).is_ok())
    }

    fn answer_quiz(&self, id: QuizId, answers: Vec<String>) -> bool {
        let mut quizzes = self.inner.quizzes.lock().unwrap();
        let Some(index) = quizzes.iter().position(|entry| entry.id == id) else {
            return false;
        };
        quizzes.remove(index).sender.send(answers).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use futures::executor::block_on;

    use super::*;

    #[test]
    fn approval_is_resolved_by_id() {
        let broker = InMemoryInteractionBroker::default();
        let id = Uuid::from_u128(1);
        let response = broker.request_approval(id, "call_1".to_owned(), "Approve?".to_owned());

        assert_eq!(broker.pending_approvals().len(), 1);
        assert!(broker.resolve_approval(id, true));
        assert!(block_on(response));
        assert!(!broker.resolve_approval(id, false));
    }

    #[test]
    fn quiz_is_answered_by_id() {
        let broker = InMemoryInteractionBroker::default();
        let id = Uuid::from_u128(2);
        let response = broker.request_quiz(
            id,
            vec![QuizQuestion {
                question: "Color?".to_owned(),
                options: Vec::new(),
            }],
        );

        assert_eq!(broker.pending_quizzes().len(), 1);
        assert!(broker.answer_quiz(id, vec!["blue".to_owned()]));
        assert_eq!(block_on(response), vec!["blue"]);
    }

    #[test]
    fn pending_quizzes_keep_request_order_when_answered_by_id() {
        let broker = InMemoryInteractionBroker::default();
        let first_id = Uuid::from_u128(10);
        let second_id = Uuid::from_u128(5);
        let first = broker.request_quiz(
            first_id,
            vec![QuizQuestion {
                question: "First?".to_owned(),
                options: Vec::new(),
            }],
        );
        let second = broker.request_quiz(
            second_id,
            vec![QuizQuestion {
                question: "Second?".to_owned(),
                options: Vec::new(),
            }],
        );

        assert_eq!(
            broker
                .pending_quizzes()
                .iter()
                .map(|quiz| quiz.id)
                .collect::<Vec<_>>(),
            vec![first_id, second_id]
        );

        assert!(broker.answer_quiz(second_id, vec!["two".to_owned()]));
        assert_eq!(
            broker
                .pending_quizzes()
                .iter()
                .map(|quiz| quiz.id)
                .collect::<Vec<_>>(),
            vec![first_id]
        );
        assert_eq!(block_on(second), vec!["two"]);
        assert!(broker.answer_quiz(first_id, vec!["one".to_owned()]));
        assert_eq!(block_on(first), vec!["one"]);
    }

    #[test]
    fn event_wire_format_is_tagged_and_cloneable() {
        let event = ThreadEvent {
            id: Uuid::from_u128(1),
            run_id: Uuid::from_u128(2),
            agent_path: vec![Uuid::from_u128(3)],
            kind: EventKind::TextDelta {
                message_id: Uuid::from_u128(4),
                delta: "hello".to_owned(),
            },
        };

        let encoded = serde_json::to_value(event.clone()).unwrap();
        assert_eq!(encoded["kind"]["type"], "text_delta");
        assert_eq!(
            serde_json::from_value::<ThreadEvent>(encoded).unwrap(),
            event
        );
    }
}
