use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use bashkit::{Bash, Builtin, BuiltinContext, ExecResult, async_trait};
use serde_json::{Value, json};

use crate::{CommandBuiltin, CommandOutput, CommandRouter, ExecInvocation, VolumeMount};

pub(crate) const PREAMBLE: &str = include_str!("subprocess_preamble.py");
const CALLBACK_NAME: &str = "__stride_exec";
const MAX_INPUT_BYTES: usize = 16 * 1024 * 1024;
const MAX_OUTPUT_BYTES: usize = 16 * 1024 * 1024;

pub(crate) fn callback(router: Arc<CommandRouter>) -> Arc<dyn eryx::Callback> {
    let schema = eryx::Schema::try_from_value(json!({ "type": "object" }))
        .unwrap_or_else(|_| eryx::Schema::empty());
    Arc::new(
        eryx::DynamicCallback::builder(
            CALLBACK_NAME.to_string(),
            "Run a buffered command from the configured command catalog.".to_string(),
            move |args| {
                let router = router.clone();
                Box::pin(async move { Ok(run(args, router).await) })
            },
        )
        .schema(schema)
        .build(),
    )
}

async fn run(args: Value, router: Arc<CommandRouter>) -> Value {
    let request = match Request::parse(args) {
        Ok(request) => request,
        Err(error) => return json!({ "error": error }),
    };
    let timeout = request.timeout_ms.map(Duration::from_millis);
    let (mut output, timed_out) = match request.command {
        RequestedCommand::Argv(argv) => {
            run_argv(router, argv, request.stdin, request.cwd, timeout).await
        }
        RequestedCommand::Line(line) => {
            run_shell(router, line, request.stdin, request.cwd, timeout).await
        }
    };
    cap_output(&mut output);
    json!({
        "returncode": output.returncode,
        "stdout_b64": base64::engine::general_purpose::STANDARD.encode(output.stdout),
        "stderr_b64": base64::engine::general_purpose::STANDARD.encode(output.stderr),
        "truncated": output.truncated,
        "timed_out": timed_out,
    })
}

async fn run_argv(
    router: Arc<CommandRouter>,
    argv: Vec<String>,
    stdin: Vec<u8>,
    cwd: String,
    timeout: Option<Duration>,
) -> (CommandOutput, bool) {
    let invocation = ExecInvocation {
        argv,
        stdin,
        cwd,
        timeout,
    };
    let command_name = invocation.argv.first().cloned().unwrap_or_default();
    let execution = router.exec_inline(invocation);
    match timeout {
        Some(limit) => match tokio::time::timeout(limit, execution).await {
            Ok(result) => (command_result(command_name, result), false),
            Err(_) => (timeout_output(), true),
        },
        None => (command_result(command_name, execution.await), false),
    }
}

fn command_result(name: String, result: anyhow::Result<CommandOutput>) -> CommandOutput {
    result.unwrap_or_else(|error| CommandOutput {
        returncode: 1,
        stderr: format!("{name}: {error:#}\n").into_bytes(),
        ..Default::default()
    })
}

async fn run_shell(
    router: Arc<CommandRouter>,
    line: String,
    stdin: Vec<u8>,
    cwd: String,
    timeout: Option<Duration>,
) -> (CommandOutput, bool) {
    let mut builder = Bash::builder().cwd(cwd);
    for mount in router.volumes() {
        builder = mount_on(builder, mount);
    }
    for (name, description) in router.catalog() {
        builder = builder.builtin(
            name,
            Box::new(CommandBuiltin::new_inline(
                router.clone(),
                name,
                description,
            )),
        );
    }
    let script = if stdin.is_empty() {
        line
    } else {
        builder = builder.builtin("__stride_stdin", Box::new(FixedInput(stdin)));
        format!("__stride_stdin | ({line})")
    };
    let mut bash = builder.build();
    let execution = bash.exec(&script);
    let result = match timeout {
        Some(limit) => match tokio::time::timeout(limit, execution).await {
            Ok(result) => result,
            Err(_) => return (timeout_output(), true),
        },
        None => execution.await,
    };
    match result {
        Ok(result) => (
            CommandOutput {
                returncode: result.exit_code,
                stdout: result.stdout.into_bytes(),
                stderr: result.stderr.into_bytes(),
                truncated: false,
            },
            false,
        ),
        Err(error) => (
            CommandOutput {
                returncode: 1,
                stderr: format!("shell: {error}\n").into_bytes(),
                ..Default::default()
            },
            false,
        ),
    }
}

fn mount_on(mut builder: bashkit::BashBuilder, mount: VolumeMount) -> bashkit::BashBuilder {
    if !mount.host_path.is_dir() {
        return builder;
    }
    if mount.read_only {
        builder = builder.mount_real_readonly_at(mount.host_path, mount.guest_path);
    } else {
        builder = builder.mount_real_readwrite_at(mount.host_path, mount.guest_path);
    }
    builder
}

struct FixedInput(Vec<u8>);

#[async_trait]
impl Builtin for FixedInput {
    async fn execute(&self, _ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        Ok(ExecResult::ok(
            String::from_utf8_lossy(&self.0).into_owned(),
        ))
    }
}

fn timeout_output() -> CommandOutput {
    CommandOutput {
        returncode: 124,
        stderr: b"command timed out\n".to_vec(),
        ..Default::default()
    }
}

fn cap_output(output: &mut CommandOutput) {
    let stdout_len = output.stdout.len().min(MAX_OUTPUT_BYTES);
    if output.stdout.len() > stdout_len {
        output.stdout.truncate(stdout_len);
        output.truncated = true;
    }
    let remaining = MAX_OUTPUT_BYTES.saturating_sub(output.stdout.len());
    if output.stderr.len() > remaining {
        output.stderr.truncate(remaining);
        output.truncated = true;
    }
}

enum RequestedCommand {
    Argv(Vec<String>),
    Line(String),
}

struct Request {
    command: RequestedCommand,
    stdin: Vec<u8>,
    cwd: String,
    timeout_ms: Option<u64>,
}

impl Request {
    fn parse(value: Value) -> Result<Self, String> {
        let object = value.as_object().ok_or("expected an object")?;
        let argv = object.get("argv").and_then(Value::as_array).map(|values| {
            values
                .iter()
                .map(|value| {
                    value
                        .as_str()
                        .map(str::to_string)
                        .ok_or_else(|| "argv entries must be strings".to_string())
                })
                .collect::<Result<Vec<_>, _>>()
        });
        let line = object
            .get("line")
            .and_then(Value::as_str)
            .map(str::to_string);
        let command = match (argv, line) {
            (Some(Ok(argv)), None) if !argv.is_empty() => RequestedCommand::Argv(argv),
            (Some(Err(error)), None) => return Err(error),
            (None, Some(line)) => RequestedCommand::Line(line),
            _ => return Err("provide exactly one non-empty argv or line".to_string()),
        };
        let command_bytes = match &command {
            RequestedCommand::Argv(argv) => argv.iter().map(String::len).sum(),
            RequestedCommand::Line(line) => line.len(),
        };
        if command_bytes > MAX_INPUT_BYTES {
            return Err("command arguments are too large".to_string());
        }
        let encoded = object
            .get("stdin_b64")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if encoded.len() > MAX_INPUT_BYTES.saturating_mul(4).div_ceil(3) + 4 {
            return Err("command input is too large".to_string());
        }
        let stdin = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|error| format!("invalid stdin_b64: {error}"))?;
        if stdin.len() > MAX_INPUT_BYTES {
            return Err("command input is too large".to_string());
        }
        let cwd = object
            .get("cwd")
            .and_then(Value::as_str)
            .unwrap_or(crate::AGENT_HOME)
            .to_string();
        if cwd.len() > MAX_INPUT_BYTES {
            return Err("command cwd is too large".to_string());
        }
        let timeout_ms = object.get("timeout_ms").and_then(Value::as_u64);
        Ok(Self {
            command,
            stdin,
            cwd,
            timeout_ms,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::{DirectOsFileSystem, ExecutionWorkspace, NativeCommand};

    struct FixtureCommand;

    #[async_trait::async_trait]
    impl NativeCommand for FixtureCommand {
        async fn run(
            &self,
            invocation: &ExecInvocation,
            mounts: &[VolumeMount],
        ) -> anyhow::Result<CommandOutput> {
            match invocation.argv.get(1).map(String::as_str) {
                Some("sleep") => {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    Ok(CommandOutput::default())
                }
                Some("copy") => {
                    let root = &mounts[0].host_path;
                    let input = tokio::fs::read(root.join(&invocation.argv[2])).await?;
                    tokio::fs::write(root.join(&invocation.argv[3]), input).await?;
                    Ok(CommandOutput::default())
                }
                _ => Ok(CommandOutput {
                    stdout: invocation.stdin.clone(),
                    ..Default::default()
                }),
            }
        }
    }

    fn router(root: &Path) -> Arc<CommandRouter> {
        let fs = DirectOsFileSystem::new(root.to_path_buf()).unwrap();
        let mut router = CommandRouter::new(Arc::new(ExecutionWorkspace::new(Arc::new(fs))));
        router.register_native("fixture", "fixture command", Arc::new(FixtureCommand));
        Arc::new(router)
    }

    #[tokio::test]
    async fn argv_protocol_carries_binary_input_and_output() {
        let workspace = tempfile::tempdir().unwrap();
        let response = run(
            json!({
                "argv": ["fixture"],
                "stdin_b64": base64::engine::general_purpose::STANDARD.encode([0, 1, 255]),
                "cwd": "/home/agent",
            }),
            router(workspace.path()),
        )
        .await;

        assert_eq!(response["returncode"], 0);
        let output = base64::engine::general_purpose::STANDARD
            .decode(response["stdout_b64"].as_str().unwrap())
            .unwrap();
        assert_eq!(output, [0, 1, 255]);
    }

    #[tokio::test]
    async fn shell_protocol_supports_pipes_and_catalog_commands() {
        let workspace = tempfile::tempdir().unwrap();
        let response = run(
            json!({
                "line": "printf hello | fixture | wc -c",
                "stdin_b64": "",
                "cwd": "/home/agent",
            }),
            router(workspace.path()),
        )
        .await;

        assert_eq!(response["returncode"], 0, "{response}");
        let output = base64::engine::general_purpose::STANDARD
            .decode(response["stdout_b64"].as_str().unwrap())
            .unwrap();
        assert_eq!(String::from_utf8(output).unwrap().trim(), "5");
    }

    #[tokio::test]
    async fn timeout_and_nested_python_are_explicit() {
        let workspace = tempfile::tempdir().unwrap();
        let router = router(workspace.path());
        let timeout = run(
            json!({
                "argv": ["fixture", "sleep"],
                "stdin_b64": "",
                "cwd": "/home/agent",
                "timeout_ms": 1,
            }),
            router.clone(),
        )
        .await;
        assert_eq!(timeout["timed_out"], true);

        let nested = run(
            json!({
                "argv": ["python", "-c", "print(1)"],
                "stdin_b64": "",
                "cwd": "/home/agent",
            }),
            router,
        )
        .await;
        assert_eq!(nested["returncode"], 126);
        let error = base64::engine::general_purpose::STANDARD
            .decode(nested["stderr_b64"].as_str().unwrap())
            .unwrap();
        assert!(
            String::from_utf8(error)
                .unwrap()
                .contains("nested python is not supported")
        );
    }

    #[tokio::test]
    async fn command_writes_are_visible_on_the_host_mount() {
        let workspace = tempfile::tempdir().unwrap();
        tokio::fs::write(workspace.path().join("input.txt"), "coherent")
            .await
            .unwrap();
        let response = run(
            json!({
                "argv": ["fixture", "copy", "input.txt", "output.txt"],
                "stdin_b64": "",
                "cwd": "/home/agent",
            }),
            router(workspace.path()),
        )
        .await;

        assert_eq!(response["returncode"], 0, "{response}");
        assert_eq!(
            tokio::fs::read_to_string(workspace.path().join("output.txt"))
                .await
                .unwrap(),
            "coherent"
        );
    }
}
