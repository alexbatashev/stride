use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use llm::Tool as LlmTool;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{AgentConfig, ToolProgressSink};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct QuizQuestion {
    pub question: String,
    /// Suggested answer options; empty means free-form answer expected.
    pub options: Vec<String>,
}

/// Format of a tool's successful, human-facing output. Drives how the result is
/// rendered to the user: markdown flows through the automarkdown widget, plain
/// text is shown verbatim, and json (the default) is treated as opaque data.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolOutputFormat {
    #[default]
    Json,
    Markdown,
    PlainText,
}

impl ToolOutputFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            ToolOutputFormat::Json => "json",
            ToolOutputFormat::Markdown => "markdown",
            ToolOutputFormat::PlainText => "plaintext",
        }
    }
}

/// Host-provided executor hook. The agent crate never names a concrete runtime;
/// the host wraps its own spawn primitive so async tools can run in the
/// background without pulling tokio into `libs/agent`. The future is `!Send`
/// because the whole agent is single-threaded (`Rc`/`RefCell`,
/// `async_trait(?Send)`), so the host must spawn onto a same-thread executor.
pub trait TaskSpawner {
    /// Spawn a `!Send` future identified by the originating tool_call_id. The
    /// task reports progress and its final result back through the channel it
    /// captured; the id lets the host retain a handle keyed by the call.
    fn spawn(&self, id: &str, future: Pin<Box<dyn Future<Output = ()> + 'static>>);
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

    /// Streaming variant of [`Tool::execute`]. Tools that produce output
    /// incrementally (subagents) override this and call [`ToolProgressSink::progress`]
    /// as they work. The default delegates to `execute` and ignores the sink,
    /// so non-streaming tools are unaffected.
    async fn execute_streaming(
        &self,
        config: Arc<AgentConfig>,
        args: Value,
        sink: ToolProgressSink,
    ) -> Value {
        let _ = sink;
        self.execute(config, args).await
    }

    /// Format of this tool's successful, human-facing output. Governs how the
    /// result is rendered to the user (see [`ToolOutputFormat`]).
    fn output_format(&self) -> ToolOutputFormat {
        ToolOutputFormat::Json
    }

    /// Whether this tool streams incremental output through
    /// [`Tool::execute_streaming`]. When true the base loop wires a progress
    /// sink so partial output reaches the user live.
    fn streams(&self) -> bool {
        false
    }

    /// Whether this tool may run asynchronously (in the background) when the
    /// model sets the injected `async` argument. Opt-in: only tools that return
    /// true expose the `async` parameter and can be spawned off the main loop.
    fn supports_async(&self) -> bool {
        false
    }

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
