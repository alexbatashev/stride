mod bash;
mod safety;

pub use bash::BashBackend;
pub use safety::command_is_read_only;

use std::sync::Arc;

use async_trait::async_trait;
use llm::{Function, Tool as LlmTool};
use serde::Serialize;
use serde_json::{Value, json};

use crate::AgentConfig;
use crate::Tool;
use crate::ToolDesc;

/// Structured outcome of running a command on a [`ShellBackend`].
#[derive(Serialize)]
pub struct ShellResult {
    pub success: bool,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub error: Option<String>,
}

impl ShellResult {
    pub fn failure(error: String) -> Self {
        ShellResult {
            success: false,
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            error: Some(error),
        }
    }
}

/// A pluggable shell implementation. Each backend decides how a command line is
/// executed and which commands are safe enough to run without user approval.
#[async_trait(?Send)]
pub trait ShellBackend: Send + Sync {
    /// Tool description shown to the model.
    fn description(&self) -> String;

    /// Run `command`, optionally starting in `working_directory`.
    async fn run(&self, command: &str, working_directory: Option<&str>) -> ShellResult;

    /// Returns true when the command needs no approval (only known-safe,
    /// side-effect-free operations).
    fn is_safe(&self, command: &str) -> bool;
}

#[derive(ToolDesc)]
struct ShellParams {
    /// Shell command to execute.
    command: String,
    /// Directory to execute the command in.
    working_directory: Option<String>,
}

pub struct ShellTool {
    backend: Box<dyn ShellBackend>,
}

impl ShellTool {
    pub fn new(backend: impl ShellBackend + 'static) -> Self {
        Self {
            backend: Box::new(backend),
        }
    }
}

#[async_trait(?Send)]
impl Tool for ShellTool {
    fn name(&self) -> &str {
        "shell"
    }

    fn readable_name(&self) -> &str {
        "Shell"
    }

    fn definition(&self) -> LlmTool {
        LlmTool {
            r#type: llm::ToolType::Function,
            function: Function {
                description: self.backend.description(),
                name: self.name().to_owned(),
                parameters: Some(ShellParams::function_parameters()),
            },
        }
    }

    async fn execute(&self, _config: Arc<AgentConfig>, args: Value) -> Value {
        let result = match ShellParams::decode(args) {
            Ok(args) => {
                self.backend
                    .run(&args.command, args.working_directory.as_deref())
                    .await
            }
            Err(error) => ShellResult::failure(error),
        };

        json!(result)
    }

    /// Approval is requested only for commands the backend deems unsafe.
    fn are_safe_args(&self, args: &Value) -> bool {
        match args.get("command").and_then(|v| v.as_str()) {
            Some(command) => self.backend.is_safe(command),
            None => false,
        }
    }

    fn confirmation_prompt(&self, args: &Value) -> String {
        match args.get("command").and_then(|v| v.as_str()) {
            Some(command) => format!("Run shell command: {command}"),
            None => "Run shell command".to_string(),
        }
    }
}
