#![allow(non_upper_case_globals)]

use minisql::{DecodeError, FromValue, IntoValue, SqlLikeType, Value, migrations};

use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    System,
    Agent,
    User,
    Tool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageFormat {
    Markdown,
    Html,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageSource {
    Human,
    Monitor,
    ToolWakeup,
}

/// Lifecycle state of an agent run. Distinct from [`RunStatus`], which tracks
/// automation runs and predates this type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentRunStatus {
    Running,
    Finished,
    Failed,
    Cancelled,
    Interrupted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolCallStatus {
    Running,
    Finished,
    Failed,
    Cancelled,
    Interrupted,
}

#[derive(Clone, Copy, Debug)]
pub enum ObjectKind {
    Directory,
    File,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutomationKind {
    Python,
    Agent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunStatus {
    Running,
    Success,
    Failed,
}

/// What causes an automation to run. Stored as a nullable text column so old
/// rows (created before triggers existed) decode as `None` and fall back to
/// `Cron`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TriggerKind {
    Cron,
    Email,
    Gmail,
    Webhook,
    Manual,
    VfsChange,
}

impl TriggerKind {
    pub fn as_str(self) -> &'static str {
        match self {
            TriggerKind::Cron => "cron",
            TriggerKind::Email => "email",
            TriggerKind::Gmail => "gmail",
            TriggerKind::Webhook => "webhook",
            TriggerKind::Manual => "manual",
            TriggerKind::VfsChange => "vfs_change",
        }
    }

    /// Parse a stored value, defaulting unknown or absent values to `Cron`.
    pub fn from_opt(value: Option<&str>) -> TriggerKind {
        match value {
            Some("webhook") => TriggerKind::Webhook,
            Some("email") => TriggerKind::Email,
            Some("gmail") => TriggerKind::Gmail,
            Some("manual") => TriggerKind::Manual,
            Some("vfs_change") => TriggerKind::VfsChange,
            _ => TriggerKind::Cron,
        }
    }
}

/// Where the result of a run is pushed, on top of the always-stored
/// conversation. Stored as nullable text; absent decodes as `None`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotifyKind {
    None,
    Telegram,
}

impl NotifyKind {
    pub fn as_str(self) -> &'static str {
        match self {
            NotifyKind::None => "none",
            NotifyKind::Telegram => "telegram",
        }
    }

    /// Parse a stored value, defaulting unknown or absent values to `None`.
    pub fn from_opt(value: Option<&str>) -> NotifyKind {
        match value {
            Some("telegram") => NotifyKind::Telegram,
            _ => NotifyKind::None,
        }
    }
}

migrations! {
    schema {
        table users {
            id: Uuid [PrimaryKey],
            username: String [Unique],
            password_hash: String,
            personality: Option<String>,
        }

        table sessions {
            id: Uuid [PrimaryKey],
            user_id: Uuid,
            expires_at: i64,

            foreign_key(user_id -> users.id);
        }

        table projects {
            id: Uuid [PrimaryKey],
            owner: Uuid,
            title: String,

            foreign_key(owner -> users.id);
        }

        table threads {
            id: Uuid [PrimaryKey],
            owner: Uuid,
            title: String,
            project_id: Option<Uuid>,

            foreign_key(owner -> users.id);
            foreign_key(project_id -> projects.id);
        }

        table messages {
            id: Uuid [PrimaryKey],
            parent_thread: Uuid,
            seq: u64,
            role: Role,
            content: String,
            thinking: Option<String>,
            tool_calls: Option<String>,
            tool_call_id: Option<String>,

            foreign_key(parent_thread -> threads.id);
        }

        table skills {
            id: Uuid [PrimaryKey],
            name: String,
            title: String,
            description: String,
            content: String,
            owner: Option<Uuid>,

            foreign_key(owner -> users.id);
        }

        raw "CREATE UNIQUE INDEX IF NOT EXISTS idx_skills_owner_name ON skills(owner, name)";

        table vfs_workspaces {
            id: Uuid [PrimaryKey],
            parent_thread: Option<Uuid>,
            parent_project: Option<Uuid>,
        }

        table vfs_nodes {
            id: Uuid [PrimaryKey],
            name: String,
            kind: ObjectKind,
            parent_node: Option<Uuid>,
            parent_workspace: Option<Uuid>,
            owner: Uuid,
            created_at: i64,
            mime_type: Option<String>,

            foreign_key(parent_node -> vfs_nodes.id);
            foreign_key(parent_workspace -> vfs_workspaces.id);
            foreign_key(owner -> users.id);
        }

        table vfs_objects {
            id: Uuid [PrimaryKey],
            version: i64,
            location: String,
            created_at: i64,
            node: Uuid,
            size: i64,

            foreign_key(node -> vfs_nodes.id);
        }
    }

    telegram_integration {
        // Retained for migration parity even though the connect-code flow was
        // replaced by the Telegram Login Widget. Migrations are append-only and
        // validated by index, so this table must stay in its original block.
        table telegram_connect_codes {
            code: String [PrimaryKey],
            user_id: Uuid [Unique],
            expires_at: i64,

            foreign_key(user_id -> users.id);
        }

        table telegram_connections {
            id: Uuid [PrimaryKey],
            user_id: Uuid [Unique],
            telegram_user_id: i64 [Unique],
            chat_id: i64,
            username: Option<String>,
            first_name: Option<String>,
            last_name: Option<String>,
            connected_at: i64,

            foreign_key(user_id -> users.id);
        }

        table telegram_threads {
            id: Uuid [PrimaryKey],
            user_id: Uuid,
            chat_id: i64,
            topic_id: i64,
            thread_id: Uuid [Unique],

            foreign_key(user_id -> users.id);
            foreign_key(thread_id -> threads.id);
        }

        table telegram_message_links {
            id: Uuid [PrimaryKey],
            user_id: Uuid,
            chat_id: i64,
            message_id: i64,
            thread_id: Uuid,

            foreign_key(user_id -> users.id);
            foreign_key(thread_id -> threads.id);
        }

        raw "CREATE UNIQUE INDEX IF NOT EXISTS idx_telegram_threads_topic ON telegram_threads(user_id, chat_id, topic_id)";
        raw "CREATE UNIQUE INDEX IF NOT EXISTS idx_telegram_message_links_message ON telegram_message_links(user_id, chat_id, message_id)";
    }

    automations {
        table automations {
            id: Uuid [PrimaryKey],
            owner: Uuid,
            name: String,
            schedule: String,
            kind: AutomationKind,
            payload: String,
            enabled: bool,
            created_at: i64,
            last_run: Option<i64>,

            foreign_key(owner -> users.id);
        }

        table automation_runs {
            id: Uuid [PrimaryKey],
            automation_id: Uuid,
            started_at: i64,
            finished_at: Option<i64>,
            status: RunStatus,
            output: String,

            foreign_key(automation_id -> automations.id);
        }
    }

    automations_triggers {
        alter table automations {
            add trigger_kind: Option<String>;
            add trigger_config: Option<String>;
            add webhook_secret: Option<String>;
            add notify_kind: Option<String>;
        }

        raw "UPDATE automations SET trigger_kind = 'cron' WHERE trigger_kind IS NULL";
        raw "UPDATE automations SET notify_kind = 'none' WHERE notify_kind IS NULL";
    }

    message_images {
        alter table messages {
            add images: Option<String>;
        }
    }

    public_images {
        table public_images {
            id: Uuid [PrimaryKey],
            token: String [Unique],
            owner: Uuid,
            location: String,
            mime_type: Option<String>,
            created_at: i64,

            foreign_key(owner -> users.id);
        }
    }

    email_integration {
        table email_accounts {
            id: Uuid [PrimaryKey],
            owner: Uuid,
            name: String,
            email: String,
            host: String,
            port: i64,
            username: String,
            password_ciphertext: String,
            inbox_mailbox: String,
            sent_mailbox: String,
            drafts_mailbox: String,
            created_at: i64,

            foreign_key(owner -> users.id);
        }

        alter table automations {
            add trigger_cursor: Option<String>;
        }

        raw "CREATE UNIQUE INDEX IF NOT EXISTS idx_email_accounts_owner_name ON email_accounts(owner, name)";
    }

    user_mcp_servers {
        table mcp_servers {
            id: Uuid [PrimaryKey],
            owner: Uuid,
            name: String,
            url: String,
            headers_json: Option<String>,
            enabled: bool,
            created_at: i64,

            foreign_key(owner -> users.id);
        }

        raw "CREATE UNIQUE INDEX IF NOT EXISTS idx_mcp_servers_owner_name ON mcp_servers(owner, name)";
    }

    github_integration {
        table github_connections {
            id: Uuid [PrimaryKey],
            user_id: Uuid [Unique],
            github_user_id: i64 [Unique],
            login: String,
            access_token: String,
            scope: Option<String>,
            connected_at: i64,

            foreign_key(user_id -> users.id);
        }

        // Short-lived CSRF tokens linking a pending OAuth flow to the user who
        // started it. The browser returns from GitHub without credentials, so the
        // signed-in user is recovered from the `state` parameter recorded here.
        table github_oauth_states {
            state: String [PrimaryKey],
            user_id: Uuid,
            expires_at: i64,

            foreign_key(user_id -> users.id);
        }
    }

    user_writable_directories {
        // Personal global directories a user has marked writable for the agent.
        // `path` is a normalized global prefix (no leading slash); the directory
        // and all of its descendants become writable in addition to the thread's
        // own workspace or project folder.
        table writable_dirs {
            id: Uuid [PrimaryKey],
            owner: Uuid,
            path: String,
            created_at: i64,

            foreign_key(owner -> users.id);
        }

        raw "CREATE UNIQUE INDEX IF NOT EXISTS idx_writable_dirs_owner_path ON writable_dirs(owner, path)";
    }

    staged_uploads {
        // Files uploaded before a thread exists. They live here until the owner
        // creates or messages a thread that references them, at which point they
        // are moved into that thread's workspace. A background sweep deletes rows
        // older than 24 hours together with their stored blobs.
        table staged_uploads {
            id: Uuid [PrimaryKey],
            owner: Uuid,
            name: String,
            mime_type: Option<String>,
            location: String,
            size: i64,
            created_at: i64,

            foreign_key(owner -> users.id);
        }
    }

    google_integration {
        // A linked Google account. Tokens are encrypted at rest with the row id
        // bound as associated data (see `crate::crypto`). `expires_at` is the
        // unix second the access token stops working; the client refreshes it
        // with the long-lived refresh token before that.
        table google_connections {
            id: Uuid [PrimaryKey],
            user_id: Uuid [Unique],
            google_user_id: String [Unique],
            email: String,
            access_token: String,
            refresh_token: String,
            scope: Option<String>,
            expires_at: i64,
            connected_at: i64,

            foreign_key(user_id -> users.id);
        }

        // Short-lived CSRF tokens linking a pending OAuth flow to the user who
        // started it. Google redirects back without credentials, so the signed-in
        // user is recovered from the `state` parameter recorded here.
        table google_oauth_states {
            state: String [PrimaryKey],
            user_id: Uuid,
            expires_at: i64,

            foreign_key(user_id -> users.id);
        }
    }

    message_content_format {
        alter table messages {
            add content_format: Option<MessageFormat>;
        }

        raw "UPDATE messages SET content_format = 'markdown' WHERE content_format IS NULL";
    }

    thread_lifecycle {
        // `last_activity_at` is the ms-since-epoch of the thread's most recent
        // message; `archived_at` is the ms-since-epoch a thread was archived
        // (NULL while active). Legacy rows keep NULL for both and fall back to
        // the thread id's embedded v7 timestamp where a value is needed.
        alter table threads {
            add last_activity_at: Option<i64>;
            add archived_at: Option<i64>;
        }
    }

    thread_retention {
        // Per-user auto-archive / auto-remove policy. A NULL day count means the
        // corresponding sweep is disabled for that user. When no row exists the
        // defaults (archive after 14 days, remove 90 days after archival) apply.
        table thread_retention_settings {
            owner: Uuid [PrimaryKey],
            archive_after_days: Option<i64>,
            remove_after_days: Option<i64>,
            updated_at: i64,

            foreign_key(owner -> users.id);
        }
    }

    user_model_providers {
        table user_providers {
            id: Uuid [PrimaryKey],
            owner: Uuid,
            name: String,
            kind: String,
            url: String,
            token_ciphertext: String,
            created_at: i64,

            foreign_key(owner -> users.id);
        }

        raw "CREATE UNIQUE INDEX IF NOT EXISTS idx_user_providers_owner_name ON user_providers(owner, name)";

        table user_models {
            id: Uuid [PrimaryKey],
            owner: Uuid,
            name: String,
            slug: String,
            provider_id: Uuid,
            reasoning_effort: Option<String>,
            vision: bool,
            created_at: i64,

            foreign_key(owner -> users.id);
            foreign_key(provider_id -> user_providers.id);
        }

        raw "CREATE UNIQUE INDEX IF NOT EXISTS idx_user_models_owner_name ON user_models(owner, name)";

        table agent_settings {
            owner: Uuid [PrimaryKey],
            subagent_allowed_models: Option<String>,
            subagent_guidelines: Option<String>,
            updated_at: i64,

            foreign_key(owner -> users.id);
        }
    }

    user_model_labels {
        alter table user_models {
            add display_name: Option<String>;
            add description: Option<String>;
        }
    }

    runs_and_tool_calls {
        alter table messages {
            add source: Option<MessageSource>;
            add run_id: Option<Uuid>;
        }

        raw "UPDATE messages SET source = 'human' WHERE role = 'user' AND source IS NULL";

        table runs {
            id: Uuid [PrimaryKey],
            thread_id: Uuid,
            status: AgentRunStatus,
            user_message_id: Option<Uuid>,
            final_message_id: Option<Uuid>,
            error: Option<String>,
            started_at_ms: i64,
            finished_at_ms: Option<i64>,

            foreign_key(thread_id -> threads.id);
        }

        table tool_call_records {
            tool_call_id: String [PrimaryKey],
            parent_thread: Uuid,
            run_id: Option<Uuid>,
            assistant_message_id: Option<Uuid>,
            tool_message_id: Option<Uuid>,
            name: String,
            status: ToolCallStatus,
            output_format: String,
            display_path: Option<String>,
            call_seq: i64,
            background: bool,
            started_at_ms: i64,
            finished_at_ms: Option<i64>,

            foreign_key(parent_thread -> threads.id);
            foreign_key(run_id -> runs.id);
            foreign_key(assistant_message_id -> messages.id);
            foreign_key(tool_message_id -> messages.id);
        }
    }
}

/// The millisecond timestamp packed into a UUIDv7's leading 48 bits.
pub fn uuid_v7_ms(id: Uuid) -> i64 {
    let bytes = id.as_bytes();
    let mut ts: u64 = 0;
    for byte in &bytes[0..6] {
        ts = (ts << 8) | u64::from(*byte);
    }
    ts as i64
}

/// Milliseconds since the Unix epoch, for run and tool-call timestamps.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

/// Deploy every schema fragment this server owns onto `db`. The core schema
/// lives in the default namespace; the memory palace is a composable fragment
/// owned by the agent crate and applied after it so its foreign keys can reach
/// the `users` table.
pub async fn migrate(
    db: &minisql::ConnectionPool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    db.migrator()
        .apply(minisql::SchemaSet::new("", get_migrations()))
        .apply(stride_agent::memory::schema())
        .run()
        .await
}

impl FromValue for Role {
    fn from_value(v: &Value) -> Result<Self, DecodeError> {
        match v {
            Value::Text(s) if s == "system" => Ok(Role::System),
            Value::Text(s) if s == "agent" => Ok(Role::Agent),
            Value::Text(s) if s == "user" => Ok(Role::User),
            Value::Text(s) if s == "tool" => Ok(Role::Tool),
            _ => Err(DecodeError("Invalid role".to_string())),
        }
    }
}

impl SqlLikeType for Role {
    fn as_sql_type() -> minisql::SqlType {
        minisql::SqlType::Text
    }
}

impl From<Role> for Value {
    fn from(val: Role) -> Value {
        match val {
            Role::System => Value::Text("system".to_string()),
            Role::Agent => Value::Text("agent".to_string()),
            Role::User => Value::Text("user".to_string()),
            Role::Tool => Value::Text("tool".to_string()),
        }
    }
}

impl IntoValue for Role {
    fn into_value(self) -> Value {
        self.into()
    }
}

impl MessageFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            MessageFormat::Markdown => "markdown",
            MessageFormat::Html => "html",
        }
    }
}

impl FromValue for MessageFormat {
    fn from_value(v: &Value) -> Result<Self, DecodeError> {
        match v {
            Value::Text(s) if s == "markdown" => Ok(MessageFormat::Markdown),
            Value::Text(s) if s == "html" => Ok(MessageFormat::Html),
            _ => Err(DecodeError("Invalid message format".to_string())),
        }
    }
}

impl SqlLikeType for MessageFormat {
    fn as_sql_type() -> minisql::SqlType {
        minisql::SqlType::Text
    }
}

impl From<MessageFormat> for Value {
    fn from(val: MessageFormat) -> Value {
        Value::Text(val.as_str().to_string())
    }
}

impl IntoValue for MessageFormat {
    fn into_value(self) -> Value {
        self.into()
    }
}

impl MessageSource {
    pub fn as_str(self) -> &'static str {
        match self {
            MessageSource::Human => "human",
            MessageSource::Monitor => "monitor",
            MessageSource::ToolWakeup => "tool_wakeup",
        }
    }
}

impl FromValue for MessageSource {
    fn from_value(v: &Value) -> Result<Self, DecodeError> {
        match v {
            Value::Text(s) if s == "human" => Ok(MessageSource::Human),
            Value::Text(s) if s == "monitor" => Ok(MessageSource::Monitor),
            Value::Text(s) if s == "tool_wakeup" => Ok(MessageSource::ToolWakeup),
            _ => Err(DecodeError("Invalid message source".to_string())),
        }
    }
}

impl SqlLikeType for MessageSource {
    fn as_sql_type() -> minisql::SqlType {
        minisql::SqlType::Text
    }
}

impl From<MessageSource> for Value {
    fn from(val: MessageSource) -> Value {
        Value::Text(val.as_str().to_string())
    }
}

impl IntoValue for MessageSource {
    fn into_value(self) -> Value {
        self.into()
    }
}

impl AgentRunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            AgentRunStatus::Running => "running",
            AgentRunStatus::Finished => "finished",
            AgentRunStatus::Failed => "failed",
            AgentRunStatus::Cancelled => "cancelled",
            AgentRunStatus::Interrupted => "interrupted",
        }
    }
}

impl FromValue for AgentRunStatus {
    fn from_value(v: &Value) -> Result<Self, DecodeError> {
        match v {
            Value::Text(s) if s == "running" => Ok(AgentRunStatus::Running),
            Value::Text(s) if s == "finished" => Ok(AgentRunStatus::Finished),
            Value::Text(s) if s == "failed" => Ok(AgentRunStatus::Failed),
            Value::Text(s) if s == "cancelled" => Ok(AgentRunStatus::Cancelled),
            Value::Text(s) if s == "interrupted" => Ok(AgentRunStatus::Interrupted),
            _ => Err(DecodeError("Invalid agent run status".to_string())),
        }
    }
}

impl SqlLikeType for AgentRunStatus {
    fn as_sql_type() -> minisql::SqlType {
        minisql::SqlType::Text
    }
}

impl From<AgentRunStatus> for Value {
    fn from(val: AgentRunStatus) -> Value {
        Value::Text(val.as_str().to_string())
    }
}

impl IntoValue for AgentRunStatus {
    fn into_value(self) -> Value {
        self.into()
    }
}

impl ToolCallStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            ToolCallStatus::Running => "running",
            ToolCallStatus::Finished => "finished",
            ToolCallStatus::Failed => "failed",
            ToolCallStatus::Cancelled => "cancelled",
            ToolCallStatus::Interrupted => "interrupted",
        }
    }
}

impl FromValue for ToolCallStatus {
    fn from_value(v: &Value) -> Result<Self, DecodeError> {
        match v {
            Value::Text(s) if s == "running" => Ok(ToolCallStatus::Running),
            Value::Text(s) if s == "finished" => Ok(ToolCallStatus::Finished),
            Value::Text(s) if s == "failed" => Ok(ToolCallStatus::Failed),
            Value::Text(s) if s == "cancelled" => Ok(ToolCallStatus::Cancelled),
            Value::Text(s) if s == "interrupted" => Ok(ToolCallStatus::Interrupted),
            _ => Err(DecodeError("Invalid tool call status".to_string())),
        }
    }
}

impl SqlLikeType for ToolCallStatus {
    fn as_sql_type() -> minisql::SqlType {
        minisql::SqlType::Text
    }
}

impl From<ToolCallStatus> for Value {
    fn from(val: ToolCallStatus) -> Value {
        Value::Text(val.as_str().to_string())
    }
}

impl IntoValue for ToolCallStatus {
    fn into_value(self) -> Value {
        self.into()
    }
}

impl SqlLikeType for ObjectKind {
    fn as_sql_type() -> minisql::SqlType {
        minisql::SqlType::Text
    }
}

impl FromValue for ObjectKind {
    fn from_value(v: &Value) -> Result<Self, DecodeError> {
        match v {
            Value::Text(s) if s == "dir" => Ok(ObjectKind::Directory),
            Value::Text(s) if s == "file" => Ok(ObjectKind::File),
            _ => Err(DecodeError("Invalid object kind".to_string())),
        }
    }
}

impl From<ObjectKind> for Value {
    fn from(val: ObjectKind) -> Value {
        match val {
            ObjectKind::Directory => Value::Text("dir".to_string()),
            ObjectKind::File => Value::Text("file".to_string()),
        }
    }
}

impl IntoValue for ObjectKind {
    fn into_value(self) -> Value {
        self.into()
    }
}

impl SqlLikeType for AutomationKind {
    fn as_sql_type() -> minisql::SqlType {
        minisql::SqlType::Text
    }
}

impl FromValue for AutomationKind {
    fn from_value(v: &Value) -> Result<Self, DecodeError> {
        match v {
            Value::Text(s) if s == "python" => Ok(AutomationKind::Python),
            Value::Text(s) if s == "agent" => Ok(AutomationKind::Agent),
            _ => Err(DecodeError("Invalid automation kind".to_string())),
        }
    }
}

impl From<AutomationKind> for Value {
    fn from(val: AutomationKind) -> Value {
        match val {
            AutomationKind::Python => Value::Text("python".to_string()),
            AutomationKind::Agent => Value::Text("agent".to_string()),
        }
    }
}

impl IntoValue for AutomationKind {
    fn into_value(self) -> Value {
        self.into()
    }
}

impl SqlLikeType for RunStatus {
    fn as_sql_type() -> minisql::SqlType {
        minisql::SqlType::Text
    }
}

impl FromValue for RunStatus {
    fn from_value(v: &Value) -> Result<Self, DecodeError> {
        match v {
            Value::Text(s) if s == "running" => Ok(RunStatus::Running),
            Value::Text(s) if s == "success" => Ok(RunStatus::Success),
            Value::Text(s) if s == "failed" => Ok(RunStatus::Failed),
            _ => Err(DecodeError("Invalid run status".to_string())),
        }
    }
}

impl From<RunStatus> for Value {
    fn from(val: RunStatus) -> Value {
        match val {
            RunStatus::Running => Value::Text("running".to_string()),
            RunStatus::Success => Value::Text("success".to_string()),
            RunStatus::Failed => Value::Text("failed".to_string()),
        }
    }
}

impl IntoValue for RunStatus {
    fn into_value(self) -> Value {
        self.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use minisql::ConnectionPool;

    /// A 41bff3a-era database has every block applied except the never-deployed
    /// `runs_and_tool_calls` tail. Applying the full set on top of that baseline
    /// must migrate cleanly (fresh alters and new tables only).
    #[tokio::test]
    async fn appends_runs_block_onto_predeploy_baseline() {
        let db = ConnectionPool::new("sqlite::memory:").unwrap();

        let mut baseline = get_migrations();
        baseline.pop();
        db.initialize_database(baseline).await.unwrap();

        db.initialize_database(get_migrations()).await.unwrap();

        db.query("INSERT INTO users (id, username, password_hash) VALUES (x'00', 'u', 'h')")
            .await
            .unwrap();
        db.query("INSERT INTO threads (id, owner, title) VALUES (x'01', x'00', 't')")
            .await
            .unwrap();
        db.query(
            "INSERT INTO runs (id, thread_id, status, started_at_ms) \
             VALUES (x'02', x'01', 'running', 1)",
        )
        .await
        .unwrap();
        db.query(
            "INSERT INTO tool_call_records \
             (tool_call_id, parent_thread, name, status, output_format, call_seq, background, started_at_ms) \
             VALUES ('c', x'01', 'n', 'running', 'json', 0, 0, 1)",
        )
        .await
        .unwrap();
        db.query(
            "INSERT INTO messages (id, parent_thread, seq, role, content, run_id) \
             VALUES (x'03', x'01', 0, 'user', 'hi', x'02')",
        )
        .await
        .unwrap();
    }
}
