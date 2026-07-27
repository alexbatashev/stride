use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use llm::{Function, Tool as LlmTool};
use serde_json::{Value as JsonValue, json};
use stride_agent::{
    AgentConfig, Tool, ToolDesc,
    tools::shell::{ShellBackend, ShellResult},
};
use uuid::Uuid;

use crate::{
    skills::{
        SYSTEM_SKILLS_HOME, Skill, SkillOrigin, SkillStore, SkillStoreError, USER_SKILLS_HOME,
    },
    vfs::{AGENT_HOME, USER_HOME},
};

const COMMAND_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_COMMAND_OUTPUT: usize = 256 * 1024;
const MAX_RENDERED_SKILL: usize = 1024 * 1024;
const MAX_ERROR_OUTPUT: usize = 8 * 1024;

pub struct SearchSkillsTool {
    pub store: Arc<SkillStore>,
    pub user_id: Uuid,
    pub excluded_system_skills: Vec<String>,
}

pub struct LoadSkillTool {
    pub store: Arc<SkillStore>,
    pub user_id: Uuid,
    pub excluded_system_skills: Vec<String>,
    pub shell: Option<Arc<dyn ShellBackend>>,
}

#[derive(ToolDesc)]
struct SearchSkillsParams {
    /// Keywords to search for among available skills.
    query: String,
}

#[derive(ToolDesc)]
struct LoadSkillParams {
    /// The unique name of the skill to load.
    name: String,
}

pub async fn skill_catalog(
    store: &SkillStore,
    user_id: Uuid,
    excluded_system_skills: &[String],
) -> String {
    let Ok(skills) = store.available(user_id, excluded_system_skills).await else {
        return String::new();
    };
    if skills.is_empty() {
        return String::new();
    }
    let mut catalog = String::from(
        "## Skills\n\nThese skills hold instructions for specific kinds of tasks. \
         When a task matches one, call `load_skill(name)` before proceeding.\n\n",
    );
    for skill in skills {
        catalog.push_str(&format!(
            "- {}: {} (path: {})\n",
            skill.manifest.name, skill.manifest.description, skill.path
        ));
    }
    catalog
}

#[async_trait(?Send)]
impl Tool for SearchSkillsTool {
    fn name(&self) -> &str {
        "search_skills"
    }

    fn readable_name(&self) -> &str {
        "Search Skills"
    }

    fn definition(&self) -> LlmTool {
        LlmTool {
            r#type: llm::ToolType::Function,
            function: Function {
                name: self.name().to_owned(),
                description: "Search filesystem-backed skills by keyword. Returns matching names, descriptions, and mounted paths. Use load_skill to render the instructions.".to_string(),
                parameters: Some(SearchSkillsParams::function_parameters()),
            },
        }
    }

    async fn execute(&self, _config: Arc<AgentConfig>, args: JsonValue) -> JsonValue {
        let params = match SearchSkillsParams::decode(args) {
            Ok(params) => params,
            Err(error) => return json!({"error": error}),
        };
        let query = params.query.to_lowercase();
        match self
            .store
            .available(self.user_id, &self.excluded_system_skills)
            .await
        {
            Err(error) => store_error(error),
            Ok(skills) => {
                let matches: Vec<JsonValue> = skills
                    .into_iter()
                    .filter(|skill| {
                        skill.manifest.name.to_lowercase().contains(&query)
                            || skill.manifest.description.to_lowercase().contains(&query)
                    })
                    .map(|skill| {
                        json!({
                            "name": skill.manifest.name,
                            "description": skill.manifest.description,
                            "path": skill.path,
                        })
                    })
                    .collect();
                json!({"found": matches.len(), "skills": matches})
            }
        }
    }
}

#[async_trait(?Send)]
impl Tool for LoadSkillTool {
    fn name(&self) -> &str {
        "load_skill"
    }

    fn readable_name(&self) -> &str {
        "Load Skill"
    }

    fn definition(&self) -> LlmTool {
        LlmTool {
            r#type: llm::ToolType::Function,
            function: Function {
                name: self.name().to_owned(),
                description: "Load and render a skill's Markdown instructions. Path variables and dynamic shell context are resolved before the content is returned.".to_string(),
                parameters: Some(LoadSkillParams::function_parameters()),
            },
        }
    }

    async fn execute(&self, _config: Arc<AgentConfig>, args: JsonValue) -> JsonValue {
        let params = match LoadSkillParams::decode(args) {
            Ok(params) => params,
            Err(error) => return json!({"error": error}),
        };
        let skill = match self
            .store
            .load(self.user_id, &params.name, &self.excluded_system_skills)
            .await
        {
            Ok(skill) => skill,
            Err(error) => return store_error(error),
        };
        match render_skill(&skill, self.shell.as_deref()).await {
            Ok(content) => json!({
                "name": skill.manifest.name,
                "description": skill.manifest.description,
                "path": skill.path,
                "content": content,
            }),
            Err(error) => error.into_json(&skill.manifest.name),
        }
    }
}

fn store_error(error: SkillStoreError) -> JsonValue {
    json!({
        "error": {
            "kind": match error {
                SkillStoreError::StorageUnavailable => "storage_unavailable",
                SkillStoreError::Invalid(_) => "invalid_skill",
                SkillStoreError::Conflict(_) => "skill_conflict",
                SkillStoreError::NotFound(_) => "skill_not_found",
                SkillStoreError::Internal(_) => "internal",
            },
            "message": error.to_string(),
        }
    })
}

#[derive(Debug)]
enum RenderError {
    InvalidSyntax(String),
    Command {
        command: String,
        reason: &'static str,
        exit_status: Option<i32>,
        stderr: String,
    },
    TooLarge,
}

impl RenderError {
    fn into_json(self, skill: &str) -> JsonValue {
        match self {
            Self::InvalidSyntax(message) => json!({
                "error": {
                    "kind": "skill_render_failed",
                    "skill": skill,
                    "message": message,
                }
            }),
            Self::Command {
                command,
                reason,
                exit_status,
                stderr,
            } => json!({
                "error": {
                    "kind": "skill_render_failed",
                    "skill": skill,
                    "command": command,
                    "reason": reason,
                    "exit_status": exit_status,
                    "stderr": stderr,
                }
            }),
            Self::TooLarge => json!({
                "error": {
                    "kind": "skill_render_failed",
                    "skill": skill,
                    "reason": "rendered skill exceeds 1 MiB",
                }
            }),
        }
    }
}

enum Segment {
    Text(String),
    Command(String),
}

async fn render_skill(
    skill: &Skill,
    shell: Option<&dyn ShellBackend>,
) -> Result<String, RenderError> {
    render_skill_with_timeout(skill, shell, COMMAND_TIMEOUT).await
}

async fn render_skill_with_timeout(
    skill: &Skill,
    shell: Option<&dyn ShellBackend>,
    command_timeout: Duration,
) -> Result<String, RenderError> {
    let home = match skill.origin {
        SkillOrigin::System => format!("{SYSTEM_SKILLS_HOME}/{}", skill.manifest.name),
        SkillOrigin::User => format!("{USER_SKILLS_HOME}/{}", skill.manifest.name),
    };
    let substituted = skill
        .content
        .replace("%SKILL_HOME%", &home)
        .replace("%USER_HOME%", USER_HOME)
        .replace("%WORKSPACE%", AGENT_HOME);
    let segments = parse_dynamic_segments(&substituted)?;
    let mut rendered = String::new();
    for segment in segments {
        match segment {
            Segment::Text(text) => append_bounded(&mut rendered, &text)?,
            Segment::Command(command) => {
                let Some(shell) = shell else {
                    return Err(RenderError::Command {
                        command,
                        reason: "skill shell is unavailable",
                        exit_status: None,
                        stderr: String::new(),
                    });
                };
                let result = tokio::time::timeout(
                    command_timeout,
                    shell.run(command.trim(), Some(AGENT_HOME)),
                )
                .await
                .map_err(|_| RenderError::Command {
                    command: command.clone(),
                    reason: "timeout",
                    exit_status: None,
                    stderr: String::new(),
                })?;
                append_command_output(&mut rendered, command, result)?;
            }
        }
    }
    Ok(rendered)
}

fn append_command_output(
    rendered: &mut String,
    command: String,
    result: ShellResult,
) -> Result<(), RenderError> {
    if !result.success {
        let mut stderr = result.stderr;
        if let Some(error) = result.error {
            if !stderr.is_empty() {
                stderr.push('\n');
            }
            stderr.push_str(&error);
        }
        truncate_utf8(&mut stderr, MAX_ERROR_OUTPUT);
        return Err(RenderError::Command {
            command,
            reason: "non-zero exit",
            exit_status: result.exit_code,
            stderr,
        });
    }
    if result.stdout.len() > MAX_COMMAND_OUTPUT {
        return Err(RenderError::Command {
            command,
            reason: "stdout exceeds 256 KiB",
            exit_status: result.exit_code,
            stderr: String::new(),
        });
    }
    append_bounded(rendered, &result.stdout)
}

fn truncate_utf8(value: &mut String, max_bytes: usize) {
    if value.len() <= max_bytes {
        return;
    }
    let mut boundary = max_bytes;
    while !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    value.truncate(boundary);
}

fn append_bounded(rendered: &mut String, value: &str) -> Result<(), RenderError> {
    if rendered.len().saturating_add(value.len()) > MAX_RENDERED_SKILL {
        return Err(RenderError::TooLarge);
    }
    rendered.push_str(value);
    Ok(())
}

fn parse_dynamic_segments(content: &str) -> Result<Vec<Segment>, RenderError> {
    let mut segments = Vec::new();
    let mut text_start = 0;
    let bytes = content.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if is_line_start(bytes, index) && is_dynamic_fence_open(content, index) {
            let opening_end = content[index..]
                .find('\n')
                .map(|offset| index + offset + 1)
                .ok_or_else(|| {
                    RenderError::InvalidSyntax("unterminated dynamic command fence".to_string())
                })?;
            let closing = find_closing_fence(content, opening_end).ok_or_else(|| {
                RenderError::InvalidSyntax("unterminated dynamic command fence".to_string())
            })?;
            push_text(&mut segments, &content[text_start..index]);
            push_command(
                &mut segments,
                content[opening_end..closing]
                    .trim_end_matches('\n')
                    .to_string(),
            )?;
            index = closing + 3;
            if content[index..].starts_with('\n') {
                index += 1;
            }
            text_start = index;
            continue;
        }
        if content[index..].starts_with("!`")
            && (index == 0
                || content[..index]
                    .chars()
                    .next_back()
                    .is_some_and(char::is_whitespace))
        {
            let command_start = index + 2;
            let Some(end_offset) = content[command_start..].find('`') else {
                return Err(RenderError::InvalidSyntax(
                    "unterminated inline dynamic command".to_string(),
                ));
            };
            let command_end = command_start + end_offset;
            push_text(&mut segments, &content[text_start..index]);
            push_command(
                &mut segments,
                content[command_start..command_end].to_string(),
            )?;
            index = command_end + 1;
            text_start = index;
            continue;
        }
        index += content[index..]
            .chars()
            .next()
            .map(char::len_utf8)
            .unwrap_or(1);
    }
    push_text(&mut segments, &content[text_start..]);
    Ok(segments)
}

fn push_command(segments: &mut Vec<Segment>, command: String) -> Result<(), RenderError> {
    if command.trim().is_empty() {
        return Err(RenderError::InvalidSyntax(
            "dynamic command must not be empty".to_string(),
        ));
    }
    segments.push(Segment::Command(command));
    Ok(())
}

fn push_text(segments: &mut Vec<Segment>, text: &str) {
    if !text.is_empty() {
        segments.push(Segment::Text(text.to_string()));
    }
}

fn is_line_start(bytes: &[u8], index: usize) -> bool {
    index == 0 || bytes.get(index.wrapping_sub(1)) == Some(&b'\n')
}

fn is_dynamic_fence_open(content: &str, index: usize) -> bool {
    let line_end = content[index..]
        .find('\n')
        .map(|relative| index + relative)
        .unwrap_or(content.len());
    content[index..line_end].trim_end_matches('\r') == "```!"
}

fn find_closing_fence(content: &str, start: usize) -> Option<usize> {
    let mut offset = start;
    while offset < content.len() {
        let line_end = content[offset..]
            .find('\n')
            .map(|relative| offset + relative)
            .unwrap_or(content.len());
        if content[offset..line_end].trim_end_matches('\r') == "```" {
            return Some(offset);
        }
        offset = line_end.saturating_add(1);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        db,
        tools::shell::EmulatedShellBackend,
        vfs::{AnyFileProvider, LocalFileProvider, MountedVfs, Vfs},
    };
    use minisql::{ConnectionPool, Value};
    use std::time::Duration;

    struct MockShell;

    #[async_trait(?Send)]
    impl ShellBackend for MockShell {
        fn description(&self) -> String {
            String::new()
        }

        async fn run(&self, command: &str, working_directory: Option<&str>) -> ShellResult {
            assert_eq!(working_directory, Some(AGENT_HOME));
            let stdout = match command {
                "first" => "one",
                "second" => "!`first`",
                other => other,
            };
            ShellResult {
                success: true,
                exit_code: Some(0),
                stdout: stdout.to_string(),
                stderr: String::new(),
                error: None,
            }
        }

        fn is_safe(&self, _command: &str) -> bool {
            true
        }
    }

    struct FailureShell;

    #[async_trait(?Send)]
    impl ShellBackend for FailureShell {
        fn description(&self) -> String {
            String::new()
        }

        async fn run(&self, _command: &str, _working_directory: Option<&str>) -> ShellResult {
            ShellResult {
                success: false,
                exit_code: Some(7),
                stdout: String::new(),
                stderr: "é".repeat(MAX_ERROR_OUTPUT),
                error: Some("failed".to_string()),
            }
        }

        fn is_safe(&self, _command: &str) -> bool {
            true
        }
    }

    struct OversizeShell;

    #[async_trait(?Send)]
    impl ShellBackend for OversizeShell {
        fn description(&self) -> String {
            String::new()
        }

        async fn run(&self, _command: &str, _working_directory: Option<&str>) -> ShellResult {
            ShellResult {
                success: true,
                exit_code: Some(0),
                stdout: "x".repeat(MAX_COMMAND_OUTPUT + 1),
                stderr: String::new(),
                error: None,
            }
        }

        fn is_safe(&self, _command: &str) -> bool {
            true
        }
    }

    struct SlowShell;

    #[async_trait(?Send)]
    impl ShellBackend for SlowShell {
        fn description(&self) -> String {
            String::new()
        }

        async fn run(&self, _command: &str, _working_directory: Option<&str>) -> ShellResult {
            tokio::time::sleep(Duration::from_millis(50)).await;
            ShellResult {
                success: true,
                exit_code: Some(0),
                stdout: "late".to_string(),
                stderr: String::new(),
                error: None,
            }
        }

        fn is_safe(&self, _command: &str) -> bool {
            true
        }
    }

    fn skill(content: &str) -> Skill {
        Skill {
            manifest: crate::skills::SkillManifest {
                name: "demo".to_string(),
                description: "Demo".to_string(),
                license: None,
                compatibility: None,
                metadata: None,
                allowed_tools: None,
            },
            content: content.to_string(),
            path: "/usr/share/skills/demo".to_string(),
            origin: SkillOrigin::System,
        }
    }

    #[tokio::test]
    async fn substitutes_paths_and_dynamic_context_once() {
        let rendered = render_skill(
            &skill("%SKILL_HOME% %USER_HOME% %WORKSPACE% %UNKNOWN%\n!`second`"),
            Some(&MockShell),
        )
        .await
        .unwrap();
        assert_eq!(
            rendered,
            "/usr/share/skills/demo /home/user /home/agent %UNKNOWN%\n!`first`"
        );
    }

    #[tokio::test]
    async fn executes_inline_and_fenced_commands_in_order() {
        let rendered = render_skill(
            &skill("A !`first` B\n```!\nsecond\n```\nC"),
            Some(&MockShell),
        )
        .await
        .unwrap();
        assert_eq!(rendered, "A one B\n!`first`C");
    }

    #[test]
    fn command_after_non_whitespace_is_literal() {
        let segments = parse_dynamic_segments("KEY=!`first`").unwrap();
        assert!(matches!(&segments[..], [Segment::Text(text)] if text == "KEY=!`first`"));
        let segments = parse_dynamic_segments("```!bash\nfirst\n```").unwrap();
        assert!(matches!(&segments[..], [Segment::Text(text)] if text == "```!bash\nfirst\n```"));
        assert!(parse_dynamic_segments("!``").is_err());
    }

    #[tokio::test]
    async fn command_failure_is_structured_and_stderr_is_bounded_utf8() {
        let error = render_skill(&skill("!`fail`"), Some(&FailureShell))
            .await
            .unwrap_err();
        let value = error.into_json("demo");
        assert_eq!(value["error"]["skill"], "demo");
        assert_eq!(value["error"]["command"], "fail");
        assert_eq!(value["error"]["exit_status"], 7);
        assert_eq!(value["error"]["reason"], "non-zero exit");
        assert!(value["error"]["stderr"].as_str().unwrap().len() <= MAX_ERROR_OUTPUT);
    }

    #[tokio::test]
    async fn command_and_rendered_output_limits_are_enforced() {
        let command_error = render_skill(&skill("!`large`"), Some(&OversizeShell))
            .await
            .unwrap_err()
            .into_json("demo");
        assert_eq!(command_error["error"]["reason"], "stdout exceeds 256 KiB");

        let render_error = render_skill(&skill(&"x".repeat(MAX_RENDERED_SKILL + 1)), None)
            .await
            .unwrap_err()
            .into_json("demo");
        assert_eq!(
            render_error["error"]["reason"],
            "rendered skill exceeds 1 MiB"
        );
    }

    #[tokio::test]
    async fn command_timeout_fails_the_entire_render() {
        let error = render_skill_with_timeout(
            &skill("before !`slow` after"),
            Some(&SlowShell),
            Duration::from_millis(1),
        )
        .await
        .unwrap_err()
        .into_json("demo");
        assert_eq!(error["error"]["skill"], "demo");
        assert_eq!(error["error"]["command"], "slow");
        assert_eq!(error["error"]["reason"], "timeout");
    }

    #[tokio::test]
    async fn dynamic_commands_read_bundles_from_the_agent_sandbox() {
        let db = ConnectionPool::new("sqlite::memory:").unwrap();
        db.initialize_database(db::get_migrations()).await.unwrap();
        let owner = Uuid::now_v7();
        db.query_with_params(
            "INSERT INTO users (id, username, password_hash) VALUES (?, ?, ?)",
            vec![
                Value::Uuid(owner),
                Value::Text("alice".to_string()),
                Value::Text("hash".to_string()),
            ],
        )
        .await
        .unwrap();
        let storage_dir = tempfile::tempdir().unwrap();
        let provider = AnyFileProvider::Local(
            LocalFileProvider::with_id_gen(
                storage_dir.path().to_path_buf(),
                Arc::new(stride_agent::SystemIdGen),
            )
            .unwrap(),
        );
        let vfs = Arc::new(Vfs::with_clock(
            db,
            provider,
            3,
            Arc::new(stride_agent::SystemClock),
            Arc::new(stride_agent::SystemIdGen),
        ));
        let store = Arc::new(SkillStore::new(Some(vfs.clone())).unwrap());
        let skill = store
            .create(
                owner,
                "sandbox-context",
                "Read mounted bundle context.",
                "Home: %SKILL_HOME%\nSystem:\n!`cat /usr/share/skills/pdf-report/assets/report-template.typ`\nUser:\n!`cat %SKILL_HOME%/assets/value.txt`",
            )
            .await
            .unwrap();
        let catalog = skill_catalog(&store, owner, &[]).await;
        assert!(catalog.contains(
            "sandbox-context: Read mounted bundle context. (path: /home/user/skills/sandbox-context)"
        ));
        vfs.write_global(
            owner,
            "skills/sandbox-context/assets/value.txt",
            "USER_ASSET",
        )
        .await
        .unwrap();
        let workspace = vfs
            .get_or_create_workspace(Uuid::now_v7(), None, owner)
            .await
            .unwrap();
        let mounted = MountedVfs::new(vfs.clone(), owner, Some(workspace), None);
        let shell = EmulatedShellBackend::new(mounted);

        let rendered = render_skill(&skill, Some(&shell)).await.unwrap();

        assert!(rendered.contains("= Report title"));
        assert!(rendered.contains("USER_ASSET"));
        assert!(rendered.contains("/home/user/skills/sandbox-context"));
        let raw = store
            .load(owner, "sandbox-context", &[])
            .await
            .unwrap()
            .content;
        assert!(raw.contains("%SKILL_HOME%"));
        assert!(raw.contains("!`cat"));

        vfs.write_global(
            owner,
            "skills/sandbox-context/SKILL.md",
            "---\nname: sandbox-context\ndescription: Changed live.\n---\nChanged",
        )
        .await
        .unwrap();
        let refreshed_catalog = skill_catalog(&store, owner, &[]).await;
        assert!(refreshed_catalog.contains("sandbox-context: Changed live."));
    }

    #[tokio::test]
    async fn agent_shell_can_create_a_global_user_skill() {
        let db = ConnectionPool::new("sqlite::memory:").unwrap();
        db.initialize_database(db::get_migrations()).await.unwrap();
        let owner = Uuid::now_v7();
        db.query_with_params(
            "INSERT INTO users (id, username, password_hash) VALUES (?, ?, ?)",
            vec![
                Value::Uuid(owner),
                Value::Text("alice".to_string()),
                Value::Text("hash".to_string()),
            ],
        )
        .await
        .unwrap();
        let storage_dir = tempfile::tempdir().unwrap();
        let provider = AnyFileProvider::Local(
            LocalFileProvider::with_id_gen(
                storage_dir.path().to_path_buf(),
                Arc::new(stride_agent::SystemIdGen),
            )
            .unwrap(),
        );
        let vfs = Arc::new(Vfs::with_clock(
            db,
            provider,
            3,
            Arc::new(stride_agent::SystemClock),
            Arc::new(stride_agent::SystemIdGen),
        ));
        let store = Arc::new(SkillStore::new(Some(vfs.clone())).unwrap());
        store.ensure_user_root(owner).await.unwrap();
        let workspace = vfs
            .get_or_create_workspace(Uuid::now_v7(), None, owner)
            .await
            .unwrap();
        let mounted = MountedVfs::new(vfs, owner, Some(workspace), None)
            .with_writable_dirs(vec![crate::skills::USER_SKILLS_ROOT.to_string()]);
        let shell = EmulatedShellBackend::new(mounted);

        let result = shell
            .run(
                "mkdir -p /home/user/skills/shell-created && printf '%s\\n' '---' \
                 'name: shell-created' 'description: Created from the shell.' '---' '' \
                 '# Instructions' '' 'Do the thing.' \
                 > /home/user/skills/shell-created/SKILL.md",
                None,
            )
            .await;
        assert!(
            result.success,
            "out={:?} err={:?}",
            result.stdout, result.stderr
        );

        let loaded = LoadSkillTool {
            store,
            user_id: owner,
            excluded_system_skills: Vec::new(),
            shell: Some(Arc::new(shell)),
        }
        .execute(
            Arc::new(AgentConfig::default()),
            json!({"name": "shell-created"}),
        )
        .await;
        assert_eq!(loaded["description"], "Created from the shell.");
        assert!(
            loaded["content"]
                .as_str()
                .unwrap()
                .contains("Do the thing.")
        );
    }
}
