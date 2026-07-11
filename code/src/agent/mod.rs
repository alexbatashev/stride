mod local;

use std::pin::Pin;

use futures::Stream;
pub use local::LocalAgent;
use stride_agent::ThreadEvent;
use uuid::Uuid;

pub trait CodeAgent {
    async fn make_turn(&self, message: &str) -> Pin<Box<dyn Stream<Item = ThreadEvent> + 'static>>;

    fn resolve_approval(&self, approval_id: Uuid, approved: bool) -> bool;

    fn answer_quiz(&self, quiz_id: Uuid, answers: Vec<String>) -> bool;
}
