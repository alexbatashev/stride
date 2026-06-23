mod local;

use std::pin::Pin;

use futures::Stream;
pub use local::LocalAgent;
use stride_agent::{AgentError, AgentResponseChunk};

pub trait CodeAgent {
    async fn make_turn(
        &self,
        message: &str,
    ) -> Pin<Box<dyn Stream<Item = Result<AgentResponseChunk, AgentError>> + 'static>>;
}
