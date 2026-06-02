mod interp;

use std::collections::HashSet;
use std::sync::{Arc, LazyLock};

use async_trait::async_trait;
use friday_agent::tools::shell::{ShellBackend, ShellResult, command_is_read_only};
use uuid::Uuid;

use crate::vfs::Vfs;

/// Emulated, read-only commands that need no approval.
static SAFE_COMMANDS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        "echo", "ls", "cat", "pwd", "cd", "grep", "head", "tail", "wc", "test", "true", "false",
    ]
    .into_iter()
    .collect()
});

const DESCRIPTION: &str = "Execute a shell command against the workspace file system. \
A subset of bash is emulated in-process (no real shell): variables, $-expansion, \
if/for/while, command sequences (; && ||), pipes, and output redirection. \
Emulated commands: echo, ls, cat, pwd, cd, mkdir, rm, mv, cp, touch, grep, head, tail, wc, test, true, false. \
Paths are workspace-relative; / is the workspace root.";

/// Shell backend that interprets a subset of bash over the VFS workspace.
pub struct EmulatedShellBackend {
    vfs: Arc<Vfs>,
    workspace_id: Uuid,
    owner: Uuid,
}

impl EmulatedShellBackend {
    pub fn new(vfs: Arc<Vfs>, workspace_id: Uuid, owner: Uuid) -> Self {
        Self {
            vfs,
            workspace_id,
            owner,
        }
    }
}

#[async_trait(?Send)]
impl ShellBackend for EmulatedShellBackend {
    fn description(&self) -> String {
        DESCRIPTION.to_string()
    }

    async fn run(&self, command: &str, working_directory: Option<&str>) -> ShellResult {
        let cwd = working_directory.unwrap_or("/");
        match interp::run(&self.vfs, self.workspace_id, self.owner, command, cwd).await {
            Ok((stdout, stderr, code)) => ShellResult {
                success: code == 0,
                exit_code: Some(code),
                stdout,
                stderr,
                error: None,
            },
            Err(error) => ShellResult::failure(error),
        }
    }

    fn is_safe(&self, command: &str) -> bool {
        command_is_read_only(command, &SAFE_COMMANDS)
    }
}
