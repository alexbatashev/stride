use crate::AgentConfig;
use crate::Tool;
use crate::ToolDesc;
use async_trait::async_trait;
use llm::{Function, Tool as LlmTool};
use patch::{Hunk, Line, Patch};
use serde::Serialize;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct PatchTool;

#[derive(ToolDesc)]
struct PatchParams {
    /// Unified diff text to apply. Paths are resolved relative to
    /// working_directory, or current process directory when omitted.
    patch: String,
    /// Directory used as base for relative paths in the patch.
    working_directory: Option<String>,
}

#[derive(Serialize)]
struct PatchResult {
    success: bool,
    files: Vec<String>,
    error: Option<String>,
}

#[async_trait(?Send)]
impl Tool for PatchTool {
    fn name(&self) -> &str {
        "patch"
    }

    fn readable_name(&self) -> &str {
        "Apply patch"
    }

    fn definition(&self) -> LlmTool {
        LlmTool {
            r#type: llm::ToolType::Function,
            function: Function {
                description: "Apply a unified diff patch to files.".to_string(),
                name: self.name().to_owned(),
                parameters: Some(PatchParams::function_parameters()),
            },
        }
    }

    async fn execute(&self, _config: Arc<AgentConfig>, args: Value) -> Value {
        let result = match PatchParams::decode(args) {
            Ok(args) => apply_patch_tool(args),
            Err(error) => Err(error),
        };

        match result {
            Ok(files) => json!(PatchResult {
                success: true,
                files,
                error: None,
            }),
            Err(error) => json!(PatchResult {
                success: false,
                files: vec![],
                error: Some(error),
            }),
        }
    }

    fn requires_confirmation(&self) -> bool {
        true
    }
}

fn apply_patch_tool(args: PatchParams) -> Result<Vec<String>, String> {
    let base_dir = args
        .working_directory
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let patches = parse_patch(&args.patch)?;
    let mut changed_files = Vec::new();

    for patch in patches {
        let path = patch_path(&patch)?;
        let path = base_dir.join(path);

        apply_file_patch(&path, patch)?;
        changed_files.push(path.to_string_lossy().to_string());
    }

    Ok(changed_files)
}

fn parse_patch(patch: &str) -> Result<Vec<Patch<'_>>, String> {
    Patch::from_multiple(patch).map_err(|err| err.to_string())
}

fn apply_file_patch(path: &Path, patch: Patch<'_>) -> Result<(), String> {
    let mut lines = if patch.old.path == "/dev/null" {
        Vec::new()
    } else {
        split_owned_lines(
            &std::fs::read_to_string(path)
                .map_err(|err| format!("failed to read {}: {err}", path.display()))?,
        )
    };

    let mut offset: isize = 0;
    for hunk in &patch.hunks {
        let start = hunk.old_range.start.saturating_sub(1);
        let start = usize::try_from(start)
            .map_err(|err| format!("invalid hunk start for {}: {err}", path.display()))?;
        let start = start
            .checked_add_signed(offset)
            .ok_or_else(|| format!("invalid hunk offset for {}", path.display()))?;

        let old_lines = old_hunk_lines(hunk);
        let new_lines = new_hunk_lines(hunk);
        let end = start + old_lines.len();

        if lines.get(start..end) != Some(old_lines.as_slice()) {
            return Err(format!("hunk does not apply to {}", path.display()));
        }

        lines.splice(start..end, new_lines);
        offset += new_hunk_lines(hunk).len() as isize - old_lines.len() as isize;
    }

    if patch.new.path == "/dev/null" {
        std::fs::remove_file(path)
            .map_err(|err| format!("failed to delete {}: {err}", path.display()))?;
    } else {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|err| format!("failed to create {}: {err}", parent.display()))?;
        }

        let mut content = lines.join("\n");
        if patch.end_newline {
            content.push('\n');
        }

        std::fs::write(path, content)
            .map_err(|err| format!("failed to write {}: {err}", path.display()))?;
    }

    Ok(())
}

fn patch_path(patch: &Patch<'_>) -> Result<String, String> {
    let path = if patch.new.path == "/dev/null" {
        &patch.old.path
    } else {
        &patch.new.path
    };

    if path == "/dev/null" {
        return Err("file patch has no path".to_string());
    }

    Ok(strip_diff_prefix(path).to_string())
}

fn strip_diff_prefix(path: &str) -> &str {
    path.strip_prefix("a/")
        .or_else(|| path.strip_prefix("b/"))
        .unwrap_or(path)
}

fn split_lines(text: &str) -> Vec<&str> {
    if text.is_empty() {
        Vec::new()
    } else if let Some(text) = text.strip_suffix('\n') {
        text.split('\n').collect()
    } else {
        text.split('\n').collect()
    }
}

fn split_owned_lines(text: &str) -> Vec<String> {
    split_lines(text).into_iter().map(str::to_string).collect()
}

fn old_hunk_lines(hunk: &Hunk) -> Vec<String> {
    hunk.lines
        .iter()
        .filter_map(|line| match line {
            Line::Context(line) | Line::Remove(line) => Some(line.to_string()),
            Line::Add(_) => None,
        })
        .collect()
}

fn new_hunk_lines(hunk: &Hunk) -> Vec<String> {
    hunk.lines
        .iter()
        .filter_map(|line| match line {
            Line::Context(line) | Line::Add(line) => Some(line.to_string()),
            Line::Remove(_) => None,
        })
        .collect()
}
