use friday_agent::{AgentConfig, ModelRegistry, Tool, tools::patch::PatchTool};
use serde_json::json;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

static TEST_ID: AtomicU64 = AtomicU64::new(0);

fn dummy_config() -> Arc<AgentConfig> {
    Arc::new(AgentConfig {
        model_registry: ModelRegistry::new(),
        max_iterations: 50,
    })
}

fn temp_dir() -> PathBuf {
    let id = TEST_ID.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("friday-agent-patch-test-{}", id));
    fs::create_dir(&dir).unwrap();
    dir
}

#[test]
fn execute_applies_unified_diff_to_existing_file() {
    let dir = temp_dir();
    let file = dir.join("example.txt");
    fs::write(&file, "one\ntwo\nthree\n").unwrap();

    let result = futures::executor::block_on(PatchTool.execute(
        dummy_config(),
        json!({
            "working_directory": dir.to_str().unwrap(),
            "patch": "\
--- a/example.txt
+++ b/example.txt
@@ -1,3 +1,3 @@
 one
-two
+second
 three
"
        }),
    ));

    assert_eq!(
        result,
        json!({
            "success": true,
            "files": [file.to_string_lossy()],
            "error": null
        })
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), "one\nsecond\nthree\n");

    fs::remove_file(&file).unwrap();
    fs::remove_dir(&dir).unwrap();
}

#[test]
fn execute_creates_and_deletes_files() {
    let dir = temp_dir();
    let created = dir.join("created.txt");
    let deleted = dir.join("deleted.txt");
    fs::write(&deleted, "remove me\n").unwrap();

    let result = futures::executor::block_on(PatchTool.execute(
        dummy_config(),
        json!({
            "working_directory": dir.to_str().unwrap(),
            "patch": "\
--- /dev/null
+++ b/created.txt
@@ -0,0 +1 @@
+hello
--- a/deleted.txt
+++ /dev/null
@@ -1 +0,0 @@
-remove me
"
        }),
    ));

    assert_eq!(result["success"], true);
    assert_eq!(fs::read_to_string(&created).unwrap(), "hello\n");
    assert!(!deleted.exists());

    fs::remove_file(&created).unwrap();
    fs::remove_dir(&dir).unwrap();
}

#[test]
fn execute_returns_error_when_hunk_does_not_match() {
    let dir = temp_dir();
    let file = dir.join("example.txt");
    fs::write(&file, "actual\n").unwrap();

    let result = futures::executor::block_on(PatchTool.execute(
        dummy_config(),
        json!({
            "working_directory": dir.to_str().unwrap(),
            "patch": "\
--- a/example.txt
+++ b/example.txt
@@ -1 +1 @@
-expected
+changed
"
        }),
    ));

    assert_eq!(result["success"], false);
    assert_eq!(fs::read_to_string(&file).unwrap(), "actual\n");

    fs::remove_file(&file).unwrap();
    fs::remove_dir(&dir).unwrap();
}
