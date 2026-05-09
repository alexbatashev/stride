mod local;

use std::pin::Pin;

use friday_agent::{AgentError, AgentResponseChunk};
use futures::Stream;
pub use local::LocalAgent;

pub struct Message {}

pub trait CodeAgent {
    fn get_messages(&self) -> Vec<Message>;
    async fn make_turn()
    -> Pin<Box<dyn Stream<Item = Result<AgentResponseChunk, AgentError>> + 'static>>;
}
