use serde_json::json;
use std::fs;
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::time::{SystemTime, UNIX_EPOCH};
use stride_agent::{AgentConfig, ModelRegistry, Tool, tools::file::ReadFileTool};

fn dummy_config() -> Arc<AgentConfig> {
    Arc::new(AgentConfig {
        model_registry: ModelRegistry::new(),
        max_iterations: 50,
    })
}

fn temp_dir() -> PathBuf {
    static NEXT_ID: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "stride-agent-file-test-{}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT_ID.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&dir).unwrap();
    dir
}

#[test]
fn execute_reads_utf8_file() {
    let dir = temp_dir();
    let file = dir.join("example.txt");
    fs::write(&file, "hello\nworld\n").unwrap();

    let result = futures::executor::block_on(ReadFileTool.execute(
        dummy_config(),
        json!({
            "path": file.to_str().unwrap()
        }),
    ));

    assert_eq!(
        result,
        json!({
            "success": true,
            "content": "hello\nworld\n",
            "error": null
        })
    );

    fs::remove_file(&file).unwrap();
    fs::remove_dir(&dir).unwrap();
}

#[test]
fn execute_returns_error_when_file_is_missing() {
    let dir = temp_dir();
    let file = dir.join("missing.txt");

    let result = futures::executor::block_on(ReadFileTool.execute(
        dummy_config(),
        json!({
            "path": file.to_str().unwrap()
        }),
    ));

    assert_eq!(result["success"], false);
    assert_eq!(result["content"], json!(null));
    assert!(result["error"].as_str().unwrap().contains("failed to read"));

    fs::remove_dir(&dir).unwrap();
}
