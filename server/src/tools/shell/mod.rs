mod vfs_backend;

use std::collections::HashSet;
use std::sync::{Arc, LazyLock};

use async_trait::async_trait;
use bashkit::{Bash, PosixFs};
use friday_agent::tools::shell::{ShellBackend, ShellResult, command_is_read_only};

use crate::vfs::MountedVfs;
use vfs_backend::VfsBackend;

/// Read-only commands that need no approval.
static SAFE_COMMANDS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        "echo", "ls", "cat", "pwd", "cd", "grep", "head", "tail", "wc", "test", "true", "false",
    ]
    .into_iter()
    .collect()
});

/// Working directory the shell starts in: the writable workspace mount.
const DEFAULT_CWD: &str = "/~workspace";

const DESCRIPTION: &str = "Execute a shell command against the workspace file system. \
Commands run in an in-process bash sandbox (bashkit) over the virtual file system; no real shell or host access. \
Supports variables, expansion, pipes, redirection, control flow, and the usual coreutils. \
The working directory is the writable workspace at /~workspace. Your read-only files live under / alongside it; \
writes outside /~workspace are rejected.";

/// Shell backend that runs bashkit over the mounted VFS.
pub struct EmulatedShellBackend {
    fs: MountedVfs,
}

impl EmulatedShellBackend {
    pub fn new(fs: MountedVfs) -> Self {
        Self { fs }
    }
}

#[async_trait(?Send)]
impl ShellBackend for EmulatedShellBackend {
    fn description(&self) -> String {
        DESCRIPTION.to_string()
    }

    async fn run(&self, command: &str, working_directory: Option<&str>) -> ShellResult {
        let cwd = working_directory.unwrap_or(DEFAULT_CWD);
        let fs: Arc<dyn bashkit::FileSystem> =
            Arc::new(PosixFs::new(VfsBackend::new(self.fs.clone())));
        let mut bash = Bash::builder().fs(fs).cwd(cwd).build();
        match bash.exec(command).await {
            Ok(result) => ShellResult {
                success: result.exit_code == 0,
                exit_code: Some(result.exit_code),
                stdout: result.stdout,
                stderr: result.stderr,
                error: None,
            },
            Err(error) => ShellResult::failure(error.to_string()),
        }
    }

    fn is_safe(&self, command: &str) -> bool {
        command_is_read_only(command, &SAFE_COMMANDS)
    }
}

#[cfg(test)]
mod tests {
    use minisql::{ConnectionPool, Value};
    use uuid::Uuid;

    use super::*;
    use crate::db;
    use crate::vfs::{AnyFileProvider, LocalFileProvider, Vfs};

    async fn backend() -> (EmulatedShellBackend, MountedVfs) {
        let db = ConnectionPool::new("sqlite::memory:").unwrap();
        db.initialize_database(db::get_migrations()).await.unwrap();

        let owner = Uuid::now_v7();
        db.query_with_params(
            "INSERT INTO users (id, username, password_hash) VALUES (?, ?, ?)",
            vec![
                Value::Uuid(owner),
                Value::Text("alice".to_string()),
                Value::Text("hash".to_string()),
            ],
        )
        .await
        .unwrap();

        let base = tempfile::tempdir().unwrap().keep();
        let storage = AnyFileProvider::Local(LocalFileProvider::new(base).unwrap());
        let vfs = Arc::new(Vfs::new(db, storage, 3));
        let ws = vfs
            .get_or_create_workspace(Uuid::now_v7(), None, owner)
            .await
            .unwrap();
        let mounted = MountedVfs::new(vfs, ws, owner);
        (EmulatedShellBackend::new(mounted.clone()), mounted)
    }

    #[tokio::test]
    async fn echo_writes_stdout() {
        let (sh, _) = backend().await;
        let result = sh.run("echo hello", None).await;
        assert!(
            result.success,
            "out={:?} err={:?}",
            result.stdout, result.stderr
        );
        assert_eq!(result.stdout, "hello\n");
    }

    #[tokio::test]
    async fn redirect_persists_to_workspace() {
        let (sh, fs) = backend().await;
        let result = sh.run("echo hi > out.txt", None).await;
        assert!(
            result.success,
            "out={:?} err={:?}",
            result.stdout, result.stderr
        );
        assert_eq!(fs.read("/~workspace/out.txt").await.unwrap(), "hi\n");
    }

    #[tokio::test]
    async fn nested_mkdir_then_write() {
        let (sh, fs) = backend().await;
        let result = sh
            .run("mkdir -p sub/deep && echo z > sub/deep/c.txt", None)
            .await;
        assert!(
            result.success,
            "out={:?} err={:?}",
            result.stdout, result.stderr
        );
        assert_eq!(fs.read("/~workspace/sub/deep/c.txt").await.unwrap(), "z\n");
    }

    #[tokio::test]
    async fn copy_file() {
        let (sh, fs) = backend().await;
        assert!(sh.run("echo data > a.txt", None).await.success);
        let result = sh.run("cp a.txt b.txt", None).await;
        assert!(
            result.success,
            "out={:?} err={:?}",
            result.stdout, result.stderr
        );
        assert_eq!(fs.read("/~workspace/b.txt").await.unwrap(), "data\n");
    }

    #[tokio::test]
    async fn pipe_into_grep() {
        let (sh, _) = backend().await;
        let result = sh.run("printf 'a\\nb\\nc\\n' | grep b", None).await;
        assert!(
            result.success,
            "out={:?} err={:?}",
            result.stdout, result.stderr
        );
        assert_eq!(result.stdout, "b\n");
    }

    #[tokio::test]
    async fn write_outside_workspace_is_rejected() {
        let (sh, _) = backend().await;
        let result = sh.run("echo nope > /forbidden.txt", None).await;
        assert!(
            !result.success,
            "out={:?} err={:?}",
            result.stdout, result.stderr
        );
    }

    #[tokio::test]
    async fn workspace_mount_is_listed_at_root() {
        let (sh, _) = backend().await;
        let result = sh.run("ls /", None).await;
        assert!(
            result.success,
            "out={:?} err={:?}",
            result.stdout, result.stderr
        );
        assert!(
            result.stdout.contains("~workspace"),
            "out={:?} err={:?}",
            result.stdout,
            result.stderr
        );
    }
}
