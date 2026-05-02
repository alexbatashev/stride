use friday_agent::{Tool, tools::file::ReadFileTool};
use serde_json::json;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "friday-agent-file-test-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&dir).unwrap();
    dir
}

#[test]
fn execute_reads_utf8_file() {
    let dir = temp_dir();
    let file = dir.join("example.txt");
    fs::write(&file, "hello\nworld\n").unwrap();

    let result = futures::executor::block_on(ReadFileTool.execute(json!({
        "path": file.to_str().unwrap()
    })));

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

    let result = futures::executor::block_on(ReadFileTool.execute(json!({
        "path": file.to_str().unwrap()
    })));

    assert_eq!(result["success"], false);
    assert_eq!(result["content"], json!(null));
    assert!(result["error"].as_str().unwrap().contains("failed to read"));

    fs::remove_dir(&dir).unwrap();
}
