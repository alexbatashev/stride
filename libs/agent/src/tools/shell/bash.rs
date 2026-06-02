use std::collections::HashSet;
use std::process::Command;
use std::sync::LazyLock;

use async_trait::async_trait;

use super::{ShellBackend, ShellResult, command_is_read_only};

/// Read-only commands that never need approval.
static SAFE_COMMANDS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        "ls", "cat", "echo", "pwd", "grep", "find", "head", "tail", "wc", "sort", "uniq", "cut",
        "tr", "rev", "nl", "which", "whoami", "id", "groups", "date", "env", "printenv",
        "basename", "dirname", "realpath", "readlink", "stat", "file", "du", "df", "tree", "diff",
        "cmp", "uname", "hostname", "true", "false", "test", "printf", "seq", "yes",
    ]
    .into_iter()
    .collect()
});

/// Shell backend that runs commands through the host's real `sh`/`cmd`.
pub struct BashBackend;

#[async_trait(?Send)]
impl ShellBackend for BashBackend {
    fn description(&self) -> String {
        "Execute a shell command.".to_string()
    }

    async fn run(&self, command: &str, working_directory: Option<&str>) -> ShellResult {
        let mut shell = shell_command(command);
        if let Some(dir) = working_directory {
            shell.current_dir(dir);
        }

        match shell.output() {
            Ok(output) => ShellResult {
                success: output.status.success(),
                exit_code: output.status.code(),
                stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                error: None,
            },
            Err(err) => ShellResult::failure(format!("failed to execute command: {err}")),
        }
    }

    fn is_safe(&self, command: &str) -> bool {
        command_is_read_only(command, &SAFE_COMMANDS)
    }
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
