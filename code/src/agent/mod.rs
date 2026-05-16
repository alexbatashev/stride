mod local;

use std::pin::Pin;

use friday_agent::{AgentError, AgentResponseChunk};
use futures::Stream;
pub use local::LocalAgent;

pub trait CodeAgent {
    async fn make_turn(
        &self,
        message: &str,
    ) -> Pin<Box<dyn Stream<Item = Result<AgentResponseChunk, AgentError>> + 'static>>;
}
