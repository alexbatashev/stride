//! Bashkit `python` / `python3` builtin backed by an execenv [`ExecutionService`].
//!
//! Registering this on a `bashkit::Bash` overrides the (feature-gated, Monty
//! based) builtin so shell scripts run the full CPython sandbox instead:
//!
//! ```bash
//! python /~workspace/script.py
//! python ./script.py
//! python -c "print('hi')"
//! echo "print('hi')" | python
//! ```
//!
//! The script source is read from the shell's virtual filesystem and forwarded
//! verbatim to the interpreter. File I/O performed by the script targets the
//! interpreter's own volumes (e.g. the `/~workspace` mount), not the bashkit VFS.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use bashkit::{Builtin, BuiltinContext, ExecResult, async_trait};

use crate::{ExecutionOutput, ExecutionService};

const USAGE: &str = "usage: python [-c cmd | file | -] [arg ...]\n\
     Options:\n  \
     -c cmd : execute code from string\n  \
     file   : execute code from a file in the workspace\n  \
     -      : read code from stdin\n  \
     -V     : print version\n";

/// Runs Python through an [`ExecutionService`]. Share one service between this
/// and the agent's `python` tool to reuse the runtime and workspace sync.
pub struct PythonBuiltin {
    service: Arc<dyn ExecutionService>,
}

impl PythonBuiltin {
    pub fn new(service: Arc<dyn ExecutionService>) -> Self {
        Self { service }
    }
}

#[async_trait]
impl Builtin for PythonBuiltin {
    async fn execute(&self, ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        let code = match resolve_source(&ctx).await {
            Source::Run(code) => code,
            Source::Empty => return Ok(ExecResult::ok(String::new())),
            Source::Reply(result) => return Ok(result),
        };

        match self.service.execute_python(&code).await {
            Ok(output) => Ok(into_result(output)),
            Err(err) => Ok(ExecResult::err(format!("python: {err:#}\n"), 1)),
        }
    }

    fn llm_hint(&self) -> Option<&'static str> {
        Some(
            "python/python3: run a Python script from the workspace \
             (python path/to/script.py), inline code (python -c '...'), or stdin. \
             Full CPython sandbox; /~workspace is writable.",
        )
    }
}

/// What to do after argument parsing.
enum Source {
    /// Execute this code.
    Run(String),
    /// Nothing to run (empty piped stdin); succeed silently.
    Empty,
    /// A ready response (version, usage, or an error) — return it as-is.
    Reply(ExecResult),
}

async fn resolve_source(ctx: &BuiltinContext<'_>) -> Source {
    match ctx.args.first().map(String::as_str) {
        Some("--version" | "-V") => Source::Reply(ExecResult::ok("Python 3.14 (execenv)\n")),
        Some("--help" | "-h") => Source::Reply(ExecResult::ok(USAGE)),
        Some("-c") => match ctx.args.get(1) {
            Some(code) if !code.is_empty() => Source::Run(code.clone()),
            _ => Source::Reply(ExecResult::err("python: option -c requires argument\n", 2)),
        },
        Some("-") => match ctx.stdin {
            Some(input) if !input.is_empty() => Source::Run(input.to_string()),
            _ => Source::Reply(ExecResult::err("python: no input from stdin\n", 1)),
        },
        Some(opt) if opt.starts_with('-') => Source::Reply(ExecResult::err(
            format!("python: unknown option: {opt}\n"),
            2,
        )),
        Some(script_path) => read_script(ctx, script_path).await,
        None => match ctx.stdin {
            Some(input) if !input.is_empty() => Source::Run(input.to_string()),
            Some(_) => Source::Empty,
            None => Source::Reply(ExecResult::err(
                "python: interactive mode not supported\n",
                1,
            )),
        },
    }
}

async fn read_script(ctx: &BuiltinContext<'_>, script_path: &str) -> Source {
    let path = resolve_path(ctx.cwd, script_path);
    match ctx.fs.read_file(&path).await {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(code) => Source::Run(code),
            Err(_) => Source::Reply(ExecResult::err(
                format!("python: can't decode file '{script_path}': not UTF-8\n"),
                1,
            )),
        },
        Err(_) => Source::Reply(ExecResult::err(
            format!("python: can't open file '{script_path}': No such file or directory\n"),
            2,
        )),
    }
}

fn resolve_path(cwd: &Path, raw: &str) -> PathBuf {
    let path = Path::new(raw);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    }
}

/// The interpreter reports no process exit code, so a returned result is treated
/// as success (exit 0) and an infrastructure error (timeout, OOM) as exit 1.
fn into_result(output: ExecutionOutput) -> ExecResult {
    ExecResult {
        stdout: output.stdout,
        stderr: output.stderr,
        exit_code: 0,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bashkit::{Bash, FileSystem, InMemoryFs};

    /// Echoes the received script back as stdout so tests can assert what the
    /// builtin forwarded to the interpreter.
    struct EchoService;

    #[async_trait]
    impl ExecutionService for EchoService {
        async fn execute_python(&self, script: &str) -> anyhow::Result<ExecutionOutput> {
            Ok(ExecutionOutput {
                stdout: script.to_string(),
                stderr: String::new(),
            })
        }
    }

    fn bash_with_python(fs: Arc<dyn FileSystem>, cwd: &str) -> Bash {
        Bash::builder()
            .fs(fs)
            .cwd(cwd)
            .builtin(
                "python",
                Box::new(PythonBuiltin::new(Arc::new(EchoService))),
            )
            .builtin(
                "python3",
                Box::new(PythonBuiltin::new(Arc::new(EchoService))),
            )
            .build()
    }

    #[tokio::test]
    async fn runs_inline_code() {
        let mut bash = bash_with_python(Arc::new(InMemoryFs::new()), "/");
        let result = bash.exec("python -c \"print('hi')\"").await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "print('hi')");
    }

    #[tokio::test]
    async fn runs_absolute_script_path() {
        let fs = Arc::new(InMemoryFs::new());
        fs.mkdir(Path::new("/~workspace"), true).await.unwrap();
        fs.write_file(Path::new("/~workspace/script.py"), b"print('from file')")
            .await
            .unwrap();
        let mut bash = bash_with_python(fs, "/");
        let result = bash.exec("python /~workspace/script.py").await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "print('from file')");
    }

    #[tokio::test]
    async fn runs_relative_script_path() {
        let fs = Arc::new(InMemoryFs::new());
        fs.mkdir(Path::new("/~workspace"), true).await.unwrap();
        fs.write_file(Path::new("/~workspace/script.py"), b"relative")
            .await
            .unwrap();
        let mut bash = bash_with_python(fs, "/~workspace");
        let result = bash.exec("python ./script.py").await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "relative");
    }

    #[tokio::test]
    async fn python3_alias_works() {
        let mut bash = bash_with_python(Arc::new(InMemoryFs::new()), "/");
        let result = bash.exec("python3 -c \"print(1)\"").await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "print(1)");
    }

    #[tokio::test]
    async fn reads_code_from_stdin() {
        let mut bash = bash_with_python(Arc::new(InMemoryFs::new()), "/");
        let result = bash.exec("echo \"print(2)\" | python").await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "print(2)\n");
    }

    #[tokio::test]
    async fn missing_file_fails() {
        let mut bash = bash_with_python(Arc::new(InMemoryFs::new()), "/");
        let result = bash.exec("python /~workspace/missing.py").await.unwrap();
        assert_ne!(result.exit_code, 0);
        assert!(result.stderr.contains("can't open file"));
    }

    #[tokio::test]
    async fn version_is_reported() {
        let mut bash = bash_with_python(Arc::new(InMemoryFs::new()), "/");
        let result = bash.exec("python --version").await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("execenv"));
    }
}
