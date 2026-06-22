mod vfs_backend;

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use bashkit::{Bash, PosixFs};
use friday_agent::tools::shell::{ShellBackend, ShellResult};

use crate::vfs::MountedVfs;
use vfs_backend::VfsBackend;

/// Working directory the shell starts in: the writable workspace mount.
const DEFAULT_CWD: &str = "/~workspace";

const DESCRIPTION: &str = "Execute a bash command against the workspace file system. \
Default workdir is /~workspace. Use this tool to inspect filesystem or contents of file with \
the help of standard UNIX tools, like cat, grep, ls, rg, sed, diff, mkdir, cp and others.";

/// Shell backend that runs bashkit over the mounted VFS.
pub struct EmulatedShellBackend {
    fs: MountedVfs,
    /// When set, `python`/`python3` run through execenv's interpreter instead of
    /// bashkit's built-in (Monty) one. Shared with the agent's `python` tool so
    /// scripts see the same runtime and `/~workspace` sync.
    python: Option<Arc<dyn execenv::ExecutionService>>,
    /// When set, exposes a `typst` command. The tuple is the Typst package cache
    /// directory, the font directories to scan, and whether package downloads may
    /// use the network.
    typst: Option<(Option<PathBuf>, Vec<PathBuf>, bool)>,
}

impl EmulatedShellBackend {
    pub fn new(fs: MountedVfs) -> Self {
        Self {
            fs,
            python: None,
            typst: None,
        }
    }

    /// Expose `python`/`python3` in the shell, backed by the given interpreter.
    pub fn with_python(mut self, service: Arc<dyn execenv::ExecutionService>) -> Self {
        self.python = Some(service);
        self
    }

    /// Expose a `typst` command that compiles workspace documents. `package_cache`
    /// caches downloaded `@preview` packages; `font_paths` are scanned for extra
    /// fonts; `allow_network` gates downloads.
    pub fn with_typst(
        mut self,
        package_cache: Option<PathBuf>,
        font_paths: Vec<PathBuf>,
        allow_network: bool,
    ) -> Self {
        self.typst = Some((package_cache, font_paths, allow_network));
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
        let fs: Arc<dyn bashkit::FileSystem> =
            Arc::new(PosixFs::new(VfsBackend::new(self.fs.clone())));
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
        if let Some((package_cache, font_paths, allow_network)) = &self.typst {
            builder = builder.builtin(
                "typst",
                Box::new(execenv::TypstBuiltin::new(
                    package_cache.clone(),
                    font_paths.clone(),
                    *allow_network,
                )),
            );
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
        let mounted = MountedVfs::new(vfs, owner, crate::vfs::WritableArea::Workspace(ws));
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
            .run("echo \"print('hi')\" > /~workspace/script.py", None)
            .await;
        assert!(
            write.success,
            "out={:?} err={:?}",
            write.stdout, write.stderr
        );

        let result = sh.run("python /~workspace/script.py", None).await;
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
        assert!(sh.is_safe("rm -rf /~workspace/file"));
        assert!(sh.is_safe("echo data > out.txt"));
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
