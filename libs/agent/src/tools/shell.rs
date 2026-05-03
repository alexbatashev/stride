use crate::AgentConfig;
use crate::Tool;
use crate::ToolDesc;
use async_trait::async_trait;
use llm::{Function, Tool as LlmTool};
use serde::Serialize;
use serde_json::{Value, json};
use std::process::Command;
use std::sync::Arc;

pub struct ShellTool;

#[derive(ToolDesc)]
struct ShellParams {
    /// Shell command to execute.
    command: String,
    /// Directory to execute the command in.
    working_directory: Option<String>,
}

#[derive(Serialize)]
struct ShellResult {
    success: bool,
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
    error: Option<String>,
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
                description: "Execute a shell command.".to_string(),
                name: self.name().to_owned(),
                parameters: Some(ShellParams::function_parameters()),
            },
        }
    }

    async fn execute(&self, _config: Arc<AgentConfig>, args: Value) -> Value {
        let result = match ShellParams::decode(args) {
            Ok(args) => execute_command(args),
            Err(error) => Err(error),
        };

        match result {
            Ok(result) => json!(result),
            Err(error) => json!(ShellResult {
                success: false,
                exit_code: None,
                stdout: String::new(),
                stderr: String::new(),
                error: Some(error),
            }),
        }
    }

    fn requires_confirmation(&self) -> bool {
        true
    }
}

fn execute_command(args: ShellParams) -> Result<ShellResult, String> {
    let mut command = shell_command(&args.command);

    if let Some(working_directory) = args.working_directory {
        command.current_dir(working_directory);
    }

    let output = command
        .output()
        .map_err(|err| format!("failed to execute command: {err}"))?;

    Ok(ShellResult {
        success: output.status.success(),
        exit_code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        error: None,
    })
}

#[cfg(windows)]
fn shell_command(command: &str) -> Command {
    let mut shell = Command::new("cmd");
    shell.arg("/C").arg(command);
    shell
}

#[cfg(not(windows))]
fn shell_command(command: &str) -> Command {
    let mut shell = Command::new("sh");
    shell.arg("-c").arg(command);
    shell
}
