mod local;

use std::pin::Pin;

use friday_agent::{AgentError, AgentResponseChunk};
use futures::Stream;
pub use local::LocalAgent;

pub enum Message {
    Agent { text: String },
    User(String),
    ToolCall { name: String },
    ToolOutput(String),
}

pub trait CodeAgent {
    fn get_messages(&self) -> Vec<Message>;
    async fn make_turn(
        &self,
        message: &str,
    ) -> Pin<Box<dyn Stream<Item = Result<AgentResponseChunk, AgentError>> + 'static>>;
}
