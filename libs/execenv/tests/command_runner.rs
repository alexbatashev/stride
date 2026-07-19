#![cfg(feature = "eryx")]

use std::sync::Arc;
use std::time::Duration;

use execenv::{
    CommandOutput, CommandRouter, DirectOsFileSystem, ExecInvocation, ExecutionLimits,
    ExecutionWorkspace, NativeCommand, VolumeMount, WasiCommandRunner,
};

struct EchoNative;

#[async_trait::async_trait]
impl NativeCommand for EchoNative {
    async fn run(
        &self,
        invocation: &ExecInvocation,
        _mounts: &[VolumeMount],
    ) -> anyhow::Result<CommandOutput> {
        Ok(CommandOutput {
            stdout: invocation.stdin.clone(),
            ..Default::default()
        })
    }
}

#[tokio::test]
async fn wasi_command_writes_stdout() {
    let cache = tempfile::tempdir().unwrap();
    let module_path = cache.path().join("hello.wasm");
    let module = wat::parse_str(
        r#"
        (module
          (import "wasi_snapshot_preview1" "fd_write"
            (func $fd_write (param i32 i32 i32 i32) (result i32)))
          (memory (export "memory") 1)
          (data (i32.const 0) "\10\00\00\00\06\00\00\00")
          (data (i32.const 16) "hello\n")
          (func (export "_start")
            (drop (call $fd_write
              (i32.const 1) (i32.const 0) (i32.const 1) (i32.const 8)))))
        "#,
    )
    .unwrap();
    tokio::fs::write(&module_path, module).await.unwrap();

    let runner = WasiCommandRunner::new(cache.path().join("compiled")).unwrap();
    let command = runner.prepare_file("hello", &module_path).await.unwrap();
    let output = runner
        .run(
            &command,
            &ExecInvocation {
                argv: vec!["hello".to_string()],
                stdin: Vec::new(),
                cwd: "/".to_string(),
                timeout: Some(Duration::from_secs(1)),
            },
            &[],
        )
        .await
        .unwrap();

    assert_eq!(output.returncode, 0);
    assert_eq!(output.stdout, b"hello\n");
    assert!(output.stderr.is_empty());
}

#[tokio::test]
async fn wasi_command_stops_at_wall_clock_limit() {
    let cache = tempfile::tempdir().unwrap();
    let module_path = cache.path().join("spin.wasm");
    let module = wat::parse_str(
        r#"
        (module
          (func (export "_start")
            (loop $forever (br $forever))))
        "#,
    )
    .unwrap();
    tokio::fs::write(&module_path, module).await.unwrap();

    let runner = WasiCommandRunner::new(cache.path().join("compiled")).unwrap();
    let command = runner.prepare_file("spin", &module_path).await.unwrap();
    let error = runner
        .run(
            &command,
            &ExecInvocation {
                argv: vec!["spin".to_string()],
                stdin: Vec::new(),
                cwd: "/".to_string(),
                timeout: Some(Duration::from_millis(50)),
            },
            &[],
        )
        .await
        .unwrap_err();

    assert!(error.to_string().contains("timed out"), "{error:#}");
}

#[tokio::test]
async fn wasi_command_cannot_grow_past_memory_limit() {
    let cache = tempfile::tempdir().unwrap();
    let module_path = cache.path().join("grow.wasm");
    let module = wat::parse_str(
        r#"
        (module
          (memory 1 100)
          (func (export "_start")
            (drop (memory.grow (i32.const 10)))))
        "#,
    )
    .unwrap();
    tokio::fs::write(&module_path, module).await.unwrap();
    let runner = WasiCommandRunner::new(cache.path().join("compiled")).unwrap();
    let command = runner.prepare_file("grow", &module_path).await.unwrap();
    let error = runner
        .run_with_limits(
            &command,
            &ExecInvocation {
                argv: vec!["grow".to_string()],
                stdin: Vec::new(),
                cwd: "/".to_string(),
                timeout: Some(Duration::from_secs(1)),
            },
            &[],
            &ExecutionLimits {
                max_memory_bytes: Some(2 * 65_536),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();

    assert!(error.to_string().contains("run WASI command"), "{error:#}");
}

#[tokio::test]
async fn wasi_runner_detects_p2_components() {
    let cache = tempfile::tempdir().unwrap();
    let component_path = cache.path().join("empty-component.wasm");
    tokio::fs::write(&component_path, wat::parse_str("(component)").unwrap())
        .await
        .unwrap();
    let runner = WasiCommandRunner::new(cache.path().join("compiled")).unwrap();
    let component = runner
        .prepare_file("empty-component", &component_path)
        .await
        .unwrap();
    let error = runner
        .run(
            &component,
            &ExecInvocation {
                argv: vec!["empty-component".to_string()],
                stdin: Vec::new(),
                cwd: "/".to_string(),
                timeout: Some(Duration::from_secs(1)),
            },
            &[],
        )
        .await
        .unwrap_err();

    assert!(error.to_string().contains("wasi:cli/run"), "{error:#}");
}

#[tokio::test]
async fn unknown_command_returns_127_and_catalog() {
    let workspace = tempfile::tempdir().unwrap();
    let fs = DirectOsFileSystem::new(workspace.path().to_path_buf()).unwrap();
    let mut router = CommandRouter::new(Arc::new(ExecutionWorkspace::new(Arc::new(fs))));
    router.register_native("known", "known command", Arc::new(EchoNative));

    let output = router
        .exec(ExecInvocation {
            argv: vec!["missing".to_string()],
            stdin: Vec::new(),
            cwd: "/home/agent".to_string(),
            timeout: None,
        })
        .await;

    assert_eq!(output.returncode, 127);
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "unknown command: missing\navailable commands: known\n"
    );
    assert_eq!(router.catalog(), vec![("known", "known command")]);
}

#[tokio::test]
#[ignore = "requires the pinned Pandoc release artifact"]
async fn pandoc_prints_version() {
    let module_path = std::env::var("PANDOC_WASM").expect("PANDOC_WASM must name pandoc.wasm");
    let cache = std::env::var("PANDOC_CACHE").expect("PANDOC_CACHE must name a cache directory");
    let runner = WasiCommandRunner::new(cache).unwrap();
    let command = runner.prepare_file("pandoc", module_path).await.unwrap();
    let invocation = ExecInvocation {
        argv: vec!["pandoc".to_string(), "--version".to_string()],
        stdin: Vec::new(),
        cwd: "/".to_string(),
        timeout: Some(Duration::from_secs(30)),
    };
    let output = runner.run(&command, &invocation, &[]).await.unwrap();

    assert_eq!(
        output.returncode,
        0,
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).starts_with("pandoc "));

    let started = std::time::Instant::now();
    let second = runner.run(&command, &invocation, &[]).await.unwrap();
    eprintln!("warm pandoc invocation: {:?}", started.elapsed());
    assert_eq!(second.returncode, 0);
}

#[tokio::test]
#[ignore = "requires the pinned Pandoc release artifact"]
async fn pandoc_converts_markdown_to_html() {
    let module_path = std::env::var("PANDOC_WASM").expect("PANDOC_WASM must name pandoc.wasm");
    let cache = std::env::var("PANDOC_CACHE").expect("PANDOC_CACHE must name a cache directory");
    let workspace = tempfile::tempdir().unwrap();
    tokio::fs::write(workspace.path().join("input.md"), "# Hello\n")
        .await
        .unwrap();

    let runner = WasiCommandRunner::new(cache).unwrap();
    let command = runner.prepare_file("pandoc", module_path).await.unwrap();
    let output = runner
        .run(
            &command,
            &ExecInvocation {
                argv: vec![
                    "pandoc".to_string(),
                    "-f".to_string(),
                    "markdown".to_string(),
                    "-t".to_string(),
                    "html".to_string(),
                    "-o".to_string(),
                    "output.html".to_string(),
                    "input.md".to_string(),
                ],
                stdin: Vec::new(),
                cwd: "/work".to_string(),
                timeout: Some(Duration::from_secs(10)),
            },
            &[execenv::VolumeMount::new(workspace.path(), "/work")],
        )
        .await
        .unwrap();

    assert_eq!(
        output.returncode,
        0,
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let html = tokio::fs::read_to_string(workspace.path().join("output.html"))
        .await
        .unwrap();
    assert_eq!(html, "<h1 id=\"hello\">Hello</h1>\n");

    let docx = runner
        .run(
            &command,
            &ExecInvocation {
                argv: vec![
                    "pandoc".to_string(),
                    "-o".to_string(),
                    "output.docx".to_string(),
                    "input.md".to_string(),
                ],
                stdin: Vec::new(),
                cwd: "/work".to_string(),
                timeout: Some(Duration::from_secs(10)),
            },
            &[execenv::VolumeMount::new(workspace.path(), "/work")],
        )
        .await
        .unwrap();
    assert_eq!(
        docx.returncode,
        0,
        "{}",
        String::from_utf8_lossy(&docx.stderr)
    );
    let docx = tokio::fs::read(workspace.path().join("output.docx"))
        .await
        .unwrap();
    assert_eq!(&docx[..2], b"PK");
}
