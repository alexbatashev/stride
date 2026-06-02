use friday_agent::{
    AgentConfig, ModelRegistry, Tool,
    tools::shell::{BashBackend, ShellTool},
};
use serde_json::json;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

fn dummy_config() -> Arc<AgentConfig> {
    Arc::new(AgentConfig {
        model_registry: ModelRegistry::new(),
        max_iterations: 50,
    })
}

fn temp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "friday-agent-bash-test-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&dir).unwrap();
    dir
}

#[cfg(windows)]
fn echo_command() -> &'static str {
    "echo hello"
}

#[cfg(not(windows))]
fn echo_command() -> &'static str {
    "printf hello"
}

#[cfg(windows)]
fn fail_command() -> &'static str {
    "exit /b 7"
}

#[cfg(not(windows))]
fn fail_command() -> &'static str {
    "exit 7"
}

#[cfg(windows)]
fn file_exists_command() -> &'static str {
    "if exist example.txt (exit /b 0) else (exit /b 1)"
}

#[cfg(not(windows))]
fn file_exists_command() -> &'static str {
    "test -f example.txt"
}

#[test]
fn execute_returns_stdout() {
    let result = futures::executor::block_on(ShellTool::new(BashBackend).execute(
        dummy_config(),
        json!({
            "command": echo_command()
        }),
    ));

    assert_eq!(result["success"], true);
    assert_eq!(result["exit_code"], 0);
    assert_eq!(result["stdout"].as_str().unwrap().trim(), "hello");
    assert_eq!(result["stderr"], "");
    assert_eq!(result["error"], json!(null));
}

#[test]
fn execute_returns_nonzero_exit_status() {
    let result = futures::executor::block_on(ShellTool::new(BashBackend).execute(
        dummy_config(),
        json!({
            "command": fail_command()
        }),
    ));

    assert_eq!(result["success"], false);
    assert_eq!(result["exit_code"], 7);
    assert_eq!(result["error"], json!(null));
}

#[test]
fn execute_uses_working_directory() {
    let dir = temp_dir();
    let file = dir.join("example.txt");
    fs::write(&file, "hello").unwrap();

    let result = futures::executor::block_on(ShellTool::new(BashBackend).execute(
        dummy_config(),
        json!({
            "command": file_exists_command(),
            "working_directory": dir.to_str().unwrap()
        }),
    ));

    fs::remove_file(&file).unwrap();
    fs::remove_dir(&dir).unwrap();

    assert_eq!(result["success"], true);
    assert_eq!(result["exit_code"], 0);
}

#[test]
fn read_only_commands_skip_approval() {
    let tool = ShellTool::new(BashBackend);
    assert!(tool.are_safe_args(&json!({ "command": "ls -la" })));
    assert!(tool.are_safe_args(&json!({ "command": "cat a | grep b" })));
}

#[test]
fn mutating_commands_require_approval() {
    let tool = ShellTool::new(BashBackend);
    assert!(!tool.are_safe_args(&json!({ "command": "rm -rf /" })));
    assert!(!tool.are_safe_args(&json!({ "command": "echo hi > file" })));
}
