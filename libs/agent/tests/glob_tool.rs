use serde_json::json;
use std::fs;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use stride_agent::{AgentConfig, ModelRegistry, Tool, tools::glob::GlobTool};

fn dummy_config() -> Arc<AgentConfig> {
    Arc::new(AgentConfig {
        model_registry: ModelRegistry::new(),
        max_iterations: 50,
    })
}

#[test]
fn execute_returns_file_sizes_in_bytes() {
    let dir = std::env::temp_dir().join(format!(
        "stride-agent-glob-test-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&dir).unwrap();

    let file = dir.join("example.txt");
    fs::write(&file, b"hello").unwrap();

    let result = futures::executor::block_on(GlobTool.execute(
        dummy_config(),
        json!({
            "pattern": file.to_str().unwrap()
        }),
    ));

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
