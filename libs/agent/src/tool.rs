use async_trait::async_trait;
use llm::Tool as LlmTool;
use serde_json::Value;

#[async_trait(?Send)]
pub trait Tool: Send + Sync {
    /// Get the tool name (used for registration)
    fn name(&self) -> &str;

    fn readable_name(&self) -> &str;

    /// Get the tool definition for the LLM (OpenAI function format)
    fn definition(&self) -> LlmTool;

    /// Execute the tool with the given arguments
    async fn execute(&self, args: Value) -> Value;

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
}
