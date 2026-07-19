#![cfg(all(feature = "eryx", feature = "bashkit"))]

use std::sync::Arc;

use bashkit::{Bash, InMemoryFs};
use execenv::{
    CommandBuiltin, CommandOutput, CommandRouter, DirectOsFileSystem, ExecInvocation,
    ExecutionWorkspace, NativeCommand, VolumeMount,
};

struct EchoCommand;

#[async_trait::async_trait]
impl NativeCommand for EchoCommand {
    async fn run(
        &self,
        invocation: &ExecInvocation,
        _mounts: &[VolumeMount],
    ) -> anyhow::Result<CommandOutput> {
        Ok(CommandOutput {
            stdout: format!("{}\n", invocation.argv[1..].join(" ")).into_bytes(),
            ..Default::default()
        })
    }
}

#[tokio::test]
async fn bashkit_dispatches_registered_command() {
    let workspace_dir = tempfile::tempdir().unwrap();
    let fs = DirectOsFileSystem::new(workspace_dir.path().to_path_buf()).unwrap();
    let mut router = CommandRouter::new(Arc::new(ExecutionWorkspace::new(Arc::new(fs))));
    router.register_native("echo-native", "echo arguments", Arc::new(EchoCommand));
    let router = Arc::new(router);
    let mut bash = Bash::builder()
        .fs(Arc::new(InMemoryFs::new()))
        .builtin(
            "echo-native",
            Box::new(CommandBuiltin::new(router, "echo-native", "echo arguments")),
        )
        .build();

    let output = bash.exec("echo-native one two").await.unwrap();

    assert_eq!(output.exit_code, 0);
    assert_eq!(output.stdout, "one two\n");
}

#[cfg(feature = "typst")]
#[tokio::test]
async fn routed_typst_compiles_host_workspace_file() {
    let workspace_dir = tempfile::tempdir().unwrap();
    tokio::fs::write(workspace_dir.path().join("report.typ"), "= Routed Typst")
        .await
        .unwrap();
    let fs = DirectOsFileSystem::new(workspace_dir.path().to_path_buf()).unwrap();
    let mut router = CommandRouter::new(Arc::new(ExecutionWorkspace::new(Arc::new(fs))));
    router.register_native(
        "typst",
        execenv::TYPST_DESCRIPTION,
        Arc::new(execenv::TypstCommand::new(None, Vec::new(), false)),
    );
    let output = router
        .exec(ExecInvocation {
            argv: vec![
                "typst".to_string(),
                "compile".to_string(),
                "report.typ".to_string(),
            ],
            stdin: Vec::new(),
            cwd: "/home/agent".to_string(),
            timeout: None,
        })
        .await;

    assert_eq!(
        output.returncode,
        0,
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let pdf = tokio::fs::read(workspace_dir.path().join("report.pdf"))
        .await
        .unwrap();
    assert_eq!(&pdf[..5], b"%PDF-");
}
