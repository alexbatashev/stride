use friday_agent::{Tool, tools::glob::GlobTool};
use serde_json::json;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn execute_returns_file_sizes_in_bytes() {
    let dir = std::env::temp_dir().join(format!(
        "friday-agent-glob-test-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&dir).unwrap();

    let file = dir.join("example.txt");
    fs::write(&file, b"hello").unwrap();

    let result = futures::executor::block_on(GlobTool.execute(json!({
        "pattern": file.to_str().unwrap()
    })));

    fs::remove_file(&file).unwrap();
    fs::remove_dir(&dir).unwrap();

    assert_eq!(
        result,
        json!([{
            "filename": file.to_str().unwrap(),
            "size": 5
        }])
    );
}
