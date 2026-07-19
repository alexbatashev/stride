mod vfs_backend;

use std::sync::Arc;

use async_trait::async_trait;
use bashkit::{Bash, FileSystem, InMemoryFs, MountableFs, PosixFs};
use stride_agent::tools::shell::{ShellBackend, ShellResult};

use crate::vfs::MountedVfs;
use vfs_backend::VfsBackend;

/// Working directory the shell starts in: the writable thread workspace.
const DEFAULT_CWD: &str = "/home/agent";

/// Ephemeral scratch mount, backed by memory and never persisted to the VFS.
const TMP_MOUNT: &str = "/tmp";

const DESCRIPTION: &str = "Execute a bash command against a POSIX file system with standard \
UNIX tools (cat, grep, ls, rg, sed, diff, mkdir, cp and others). Layout: /home/agent is the \
thread workspace, read-write and the default working directory; /home/user is the user's files, \
read-only except for directories explicitly granted to this thread (which are read-write); /tmp \
is read-write scratch that is discarded when the command finishes.";

/// Shell backend that runs bashkit over the mounted VFS.
pub struct EmulatedShellBackend {
    fs: MountedVfs,
    /// When set, `python`/`python3` run through execenv's interpreter instead of
    /// bashkit's built-in (Monty) one. Shared with the agent's `python` tool so
    /// scripts see the same runtime and `/home/agent` sync.
    python: Option<Arc<dyn execenv::ExecutionService>>,
    commands: Option<Arc<execenv::CommandRouter>>,
}

impl EmulatedShellBackend {
    pub fn new(fs: MountedVfs) -> Self {
        Self {
            fs,
            python: None,
            commands: None,
        }
    }

    /// Expose `python`/`python3` in the shell, backed by the given interpreter.
    pub fn with_python(mut self, service: Arc<dyn execenv::ExecutionService>) -> Self {
        self.python = Some(service);
        self
    }

    pub fn with_commands(mut self, router: Arc<execenv::CommandRouter>) -> Self {
        self.commands = Some(router);
        self
    }
}

#[async_trait(?Send)]
impl ShellBackend for EmulatedShellBackend {
    fn description(&self) -> String {
        DESCRIPTION.to_string()
    }

    async fn run(&self, command: &str, working_directory: Option<&str>) -> ShellResult {
        let cwd = working_directory.unwrap_or(DEFAULT_CWD);
        let fs: Arc<dyn FileSystem> = Arc::new(mount_filesystem(self.fs.clone()));
        let mut builder = Bash::builder().fs(fs).cwd(cwd);
        if let Some(service) = &self.python {
            builder = builder
                .builtin(
                    "python",
                    Box::new(execenv::PythonBuiltin::new(service.clone())),
                )
                .builtin(
                    "python3",
                    Box::new(execenv::PythonBuiltin::new(service.clone())),
                );
        }
        if let Some(router) = &self.commands {
            for (name, description) in router.catalog() {
                builder = builder.builtin(
                    name,
                    Box::new(execenv::CommandBuiltin::new(
                        router.clone(),
                        name,
                        description,
                    )),
                );
            }
        }
        let mut bash = builder.build();
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

    /// The shell runs in an isolated bashkit sandbox over the VFS, with no real
    /// shell or host access and writes confined to the workspace. Nothing it can
    /// do is harmful, so every command is auto-approved.
    fn is_safe(&self, _command: &str) -> bool {
        true
    }
}

/// Builds the shell's namespace: the mounted VFS at the root with an ephemeral
/// in-memory `/tmp` on top. The `/tmp` tree lives only for this command and is
/// never written back to the VFS.
fn mount_filesystem(vfs: MountedVfs) -> MountableFs {
    let root: Arc<dyn FileSystem> = Arc::new(PosixFs::new(VfsBackend::new(vfs)));
    let mountable = MountableFs::new(root);
    mountable
        .mount(TMP_MOUNT, Arc::new(InMemoryFs::new()))
        .expect("tmp mount point is absolute");
    mountable
}

#[cfg(test)]
mod tests {
    use minisql::{ConnectionPool, Value};
    use uuid::Uuid;

    use super::*;
    use crate::db;
    use crate::vfs::{AnyFileProvider, LocalFileProvider, Vfs};

    async fn mounted_with_grant(grant: Option<String>) -> MountedVfs {
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
        let storage = AnyFileProvider::Local(
            LocalFileProvider::with_id_gen(base, Arc::new(stride_agent::SystemIdGen)).unwrap(),
        );
        let vfs = Arc::new(Vfs::with_clock(
            db,
            storage,
            3,
            Arc::new(stride_agent::SystemClock),
            Arc::new(stride_agent::SystemIdGen),
        ));
        let ws = vfs
            .get_or_create_workspace(Uuid::now_v7(), None, owner)
            .await
            .unwrap();
        if let Some(prefix) = &grant {
            vfs.create_dir_global(owner, prefix).await.unwrap();
        }
        MountedVfs::new(vfs, owner, Some(ws), grant)
    }

    async fn backend() -> (EmulatedShellBackend, MountedVfs) {
        let mounted = mounted_with_grant(None).await;
        (EmulatedShellBackend::new(mounted.clone()), mounted)
    }

    /// Echoes the received script back as stdout so the test can confirm the
    /// `python` command read the file and forwarded it to the interpreter.
    struct EchoPython;

    #[async_trait]
    impl execenv::ExecutionService for EchoPython {
        async fn execute_python(&self, script: &str) -> anyhow::Result<execenv::ExecutionOutput> {
            Ok(execenv::ExecutionOutput {
                stdout: script.to_string(),
                stderr: String::new(),
            })
        }
    }

    #[tokio::test]
    async fn python_command_runs_workspace_script() {
        let (_, mounted) = backend().await;
        let sh = EmulatedShellBackend::new(mounted).with_python(Arc::new(EchoPython));

        let write = sh
            .run("echo \"print('hi')\" > /home/agent/script.py", None)
            .await;
        assert!(
            write.success,
            "out={:?} err={:?}",
            write.stdout, write.stderr
        );

        let result = sh.run("python /home/agent/script.py", None).await;
        assert!(
            result.success,
            "out={:?} err={:?}",
            result.stdout, result.stderr
        );
        assert_eq!(result.stdout, "print('hi')\n");
    }

    #[tokio::test]
    async fn python_command_absent_without_service() {
        let (sh, _) = backend().await;
        let result = sh.run("python --version", None).await;
        assert!(
            !result.success,
            "python should be unavailable without an interpreter"
        );
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
    async fn every_command_is_auto_approved() {
        let (sh, _) = backend().await;
        assert!(sh.is_safe("ls"));
        assert!(sh.is_safe("rm -rf /home/agent/file"));
        assert!(sh.is_safe("echo data > out.txt"));
    }

    #[tokio::test]
    async fn home_is_listed_at_root() {
        let (sh, _) = backend().await;
        let result = sh.run("ls /", None).await;
        assert!(
            result.success,
            "out={:?} err={:?}",
            result.stdout, result.stderr
        );
        assert!(
            result.stdout.contains("home"),
            "out={:?} err={:?}",
            result.stdout,
            result.stderr
        );
    }

    #[tokio::test]
    async fn default_cwd_is_agent_home() {
        let (sh, _) = backend().await;
        let result = sh.run("pwd", None).await;
        assert!(
            result.success,
            "out={:?} err={:?}",
            result.stdout, result.stderr
        );
        assert_eq!(result.stdout, "/home/agent\n");
    }

    #[tokio::test]
    async fn write_to_agent_home_succeeds() {
        let (sh, _) = backend().await;
        let write = sh.run("echo hi > /home/agent/a.txt", None).await;
        assert!(
            write.success,
            "out={:?} err={:?}",
            write.stdout, write.stderr
        );
        let read = sh.run("cat /home/agent/a.txt", None).await;
        assert_eq!(read.stdout, "hi\n");
    }

    #[tokio::test]
    async fn write_to_user_home_is_denied() {
        let (sh, _) = backend().await;
        let result = sh.run("echo nope > /home/user/blocked.txt", None).await;
        assert!(
            !result.success,
            "out={:?} err={:?}",
            result.stdout, result.stderr
        );
        assert!(
            result.stderr.to_lowercase().contains("read-only"),
            "out={:?} err={:?}",
            result.stdout,
            result.stderr
        );
    }

    #[tokio::test]
    async fn write_to_granted_subtree_succeeds() {
        let mounted = mounted_with_grant(Some("Projects/Acme".to_string())).await;
        let sh = EmulatedShellBackend::new(mounted);
        let write = sh
            .run("echo out > /home/user/Projects/Acme/out.txt", None)
            .await;
        assert!(
            write.success,
            "out={:?} err={:?}",
            write.stdout, write.stderr
        );
        let read = sh.run("cat /home/user/Projects/Acme/out.txt", None).await;
        assert_eq!(read.stdout, "out\n");
    }

    #[tokio::test]
    async fn tmp_roundtrip_is_ephemeral() {
        let (sh, _) = backend().await;
        let session = sh
            .run(
                "mkdir /tmp/d && echo scratch > /tmp/d/f.txt && cat /tmp/d/f.txt && ls /tmp",
                None,
            )
            .await;
        assert!(
            session.success,
            "out={:?} err={:?}",
            session.stdout, session.stderr
        );
        assert!(
            session.stdout.contains("scratch"),
            "out={:?} err={:?}",
            session.stdout,
            session.stderr
        );

        let next = sh.run("cat /tmp/d/f.txt", None).await;
        assert!(
            !next.success,
            "tmp must not persist across commands: out={:?} err={:?}",
            next.stdout, next.stderr
        );
    }

    #[tokio::test]
    async fn tmp_is_listed_at_root() {
        let (sh, _) = backend().await;
        let result = sh.run("ls /", None).await;
        assert!(
            result.stdout.contains("tmp"),
            "out={:?} err={:?}",
            result.stdout,
            result.stderr
        );
    }

    #[tokio::test]
    async fn ls_long_shows_synthetic_modes() {
        let (sh, _) = backend().await;
        sh.run("echo hi > /home/agent/a.txt", None).await;
        let agent = sh.run("ls -l /home/agent", None).await;
        assert!(
            agent.stdout.contains("-rw-r--r--"),
            "out={:?} err={:?}",
            agent.stdout,
            agent.stderr
        );

        let mounted = mounted_with_grant(Some("Projects/Acme".to_string())).await;
        let granted = EmulatedShellBackend::new(mounted);
        granted
            .run("echo out > /home/user/Projects/Acme/out.txt", None)
            .await;
        let listing = granted.run("ls -l /home/user/Projects/Acme", None).await;
        assert!(
            listing.stdout.contains("-rw-rw-r--"),
            "out={:?} err={:?}",
            listing.stdout,
            listing.stderr
        );
    }
}
