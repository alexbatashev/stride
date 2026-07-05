use std::sync::Arc;

use async_trait::async_trait;
use llm::Tool as LlmTool;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::AgentConfig;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct QuizQuestion {
    pub question: String,
    /// Suggested answer options; empty means free-form answer expected.
    pub options: Vec<String>,
}

#[async_trait(?Send)]
pub trait Tool: Send + Sync {
    /// Get the tool name (used for registration)
    fn name(&self) -> &str;

    fn readable_name(&self) -> &str;

    /// Get the tool definition for the LLM (OpenAI function format)
    fn definition(&self) -> LlmTool;

    /// Optional group used when summarizing searchable tools to the model.
    fn searchable_group(&self) -> Option<String> {
        None
    }

    /// Execute the tool with the given arguments
    async fn execute(&self, config: Arc<AgentConfig>, args: Value) -> Value;

    /// Whether this tool requires confirmation before execution
    fn requires_confirmation(&self) -> bool {
        false
    }

    /// Get a description of what this tool will do (for confirmation prompts)
    fn confirmation_prompt(&self, args: &Value) -> String {
        format!("Execute {} with args: {}", self.name(), args)
    }

    /// Returns true if arguments don't require additional approval from user
    fn are_safe_args(&self, _args: &Value) -> bool {
        true
    }

    /// If this tool requires interactive user input, return the questions to ask.
    /// When Some is returned, the base agent yields AgentResponseChunk::Quiz instead
    /// of calling execute(), and the user's answers become the tool result.
    fn quiz_questions(&self, _args: &Value) -> Option<Vec<QuizQuestion>> {
        None
    }
}
