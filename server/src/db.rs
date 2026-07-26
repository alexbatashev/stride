#![allow(non_upper_case_globals)]

use minisql::{DecodeError, FromValue, IntoValue, SqlLikeType, Value, migrations};

use uuid::Uuid;

#[derive(Clone, Copy, Debug)]
pub enum Role {
    System,
    Agent,
    User,
    Tool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MessageFormat {
    Markdown,
    Html,
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
        // Personal directories a user has marked writable for the agent. `path`
        // is an absolute `/home/user/...` path; the directory and all of its
        // descendants become writable in addition to the thread's own workspace.
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

    thread_last_model {
        // Last chat model selected/used for this thread. NULL means the UI and
        // runner fall back to the default chat model.
        alter table threads {
            add last_model: Option<String>;
        }
    }

    thread_event_journal {
        // Durable journal of a thread's structural events (run lifecycle, message
        // committed, tool started/finished, approval/quiz lifecycle). Deltas are
        // deliberately not journaled. `seq` is the per-thread monotonic counter
        // owned by the thread's worker; `payload` holds the full event kind as
        // JSON so a reconnecting client's replay reconstructs identical frames.
        table thread_events {
            id: Uuid [PrimaryKey],
            thread_id: Uuid,
            run_id: Uuid,
            seq: u64,
            agent_path: Option<String>,
            kind: String,
            message_id: Option<Uuid>,
            tool_call_id: Option<String>,
            payload: Option<String>,

            foreign_key(thread_id -> threads.id);
        }

        raw "CREATE INDEX IF NOT EXISTS idx_thread_events_thread_seq ON thread_events(thread_id, seq)";
        raw "CREATE INDEX IF NOT EXISTS idx_thread_events_thread_run ON thread_events(thread_id, run_id)";
    }

    writable_dirs_absolute {
        raw "UPDATE writable_dirs SET path = '/home/user/' || path WHERE path NOT LIKE '/home/user/%'";
    }

    message_agent_path {
        // Marks a message as belonging to a subagent: `agent_path` is the
        // slash-joined UUID path (same encoding as `thread_events.agent_path`).
        // NULL = root agent. Existing rows are all root, so NULL is correct.
        alter table messages {
            add agent_path: Option<String>;
        }

        raw "CREATE INDEX IF NOT EXISTS idx_messages_thread_agent ON messages(parent_thread, agent_path)";
    }

    thread_agents_registry {
        // One row per subagent spawned in a thread. Drives the Subagents side
        // panel: human-readable `name`, `model`, live `finished`/`result`, and
        // `agent_path` (this agent's full path, slash-joined UUIDs) for nesting.
        table thread_agents {
            agent_id: Uuid [PrimaryKey],
            thread_id: Uuid,
            agent_path: String,
            parent_tool_call_id: Option<String>,
            name: String,
            model: String,
            result: Option<String>,
            finished: bool,
            created_at: i64,

            foreign_key(thread_id -> threads.id);
        }

        raw "CREATE INDEX IF NOT EXISTS idx_thread_agents_thread ON thread_agents(thread_id)";
    }

    user_full_name {
        alter table users {
            add full_name: Option<String>;
        }

        raw "UPDATE users SET full_name = username WHERE full_name IS NULL";
    }
}

/// Deploy every schema fragment this server owns onto `db`. The core schema
/// lives in the default namespace; the memory palace is a composable fragment
/// owned by the agent crate and applied after it so its foreign keys can reach
/// the `users` table.
pub async fn migrate(
    db: &minisql::ConnectionPool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    repair_reordered_migrations(db).await?;
    db.migrator()
        .apply(minisql::SchemaSet::new("", get_migrations()))
        .apply(stride_agent::memory::schema())
        .run()
        .await
}

async fn repair_reordered_migrations(
    db: &minisql::ConnectionPool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    const FIRST_REORDERED_MIGRATION: usize = 19;

    db.ensure_migrations_table().await?;
    let applied = db
        .query_with_params(
            "SELECT id, hash FROM __migrations WHERE namespace = ? ORDER BY id",
            vec![Value::Text(String::new())],
        )
        .await?;
    let applied = applied
        .rows()
        .iter()
        .map(|row| {
            Ok((
                row.get_int("id").ok_or("migration row missing id")? as usize,
                row.get_int("hash").ok_or("migration row missing hash")? as u64,
            ))
        })
        .collect::<Result<Vec<_>, Box<dyn std::error::Error + Send + Sync>>>()?;

    let migrations = get_migrations();
    if migrations.len() < FIRST_REORDERED_MIGRATION + 4
        || applied.len() < FIRST_REORDERED_MIGRATION + 3
    {
        return Ok(());
    }

    let canonical_prefix_matches = applied
        .iter()
        .take(FIRST_REORDERED_MIGRATION)
        .enumerate()
        .all(|(index, (id, hash))| *id == index && *hash == migrations[index].fingerprint());
    if !canonical_prefix_matches {
        return Ok(());
    }

    let broken_order = [20, 21, 22, 19];
    let broken_suffix_matches = applied
        .iter()
        .skip(FIRST_REORDERED_MIGRATION)
        .enumerate()
        .zip(broken_order)
        .all(|((offset, (id, hash)), migration_index)| {
            *id == FIRST_REORDERED_MIGRATION + offset
                && *hash == migrations[migration_index].fingerprint()
        });
    let broken_history_len = FIRST_REORDERED_MIGRATION
        + 3
        + usize::from(
            applied
                .get(FIRST_REORDERED_MIGRATION + 3)
                .is_some_and(|(id, hash)| {
                    *id == FIRST_REORDERED_MIGRATION + 3 && *hash == migrations[19].fingerprint()
                }),
        );
    if !broken_suffix_matches || applied.len() != broken_history_len {
        return Ok(());
    }

    db.query("SELECT agent_path FROM messages LIMIT 0").await?;
    db.query("SELECT agent_id FROM thread_agents LIMIT 0")
        .await?;
    db.query("SELECT full_name FROM users LIMIT 0").await?;

    let transaction = db.transaction().await?;
    if broken_history_len == FIRST_REORDERED_MIGRATION + 3 {
        db.query(
            "UPDATE writable_dirs SET path = '/home/user/' || path \
             WHERE path NOT LIKE '/home/user/%'",
        )
        .await?;
    }
    for (id, migration) in migrations
        .iter()
        .enumerate()
        .skip(FIRST_REORDERED_MIGRATION)
        .take(4)
    {
        db.query_with_params(
            "INSERT INTO __migrations (namespace, id, hash) VALUES (?, ?, ?) \
             ON CONFLICT (namespace, id) DO UPDATE SET hash = excluded.hash",
            vec![
                Value::Text(String::new()),
                Value::Integer(id as i64),
                Value::Integer(migration.fingerprint() as i64),
            ],
        )
        .await?;
    }
    transaction.commit().await
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
mod migration_tests {
    use minisql::{ConnectionPool, Value};

    #[test]
    fn migration_history_is_append_only() {
        let expected = [
            -1347894622293133549,
            -4907170897126509681,
            -4886173972639724748,
            -1453091702658194794,
            -1774780578273556290,
            9110239967683136765,
            -8881546444351602386,
            -490542865494759638,
            7832050965176106685,
            4489913004991258294,
            -6740296839304145313,
            4895212364422049853,
            -8425533884196383256,
            4600297217400685299,
            -8840723801095465970,
            -5044766499541310111,
            -6771270855215003650,
            921980084376497665,
            -5925523933975238023,
            7684280566407992789,
            -8100939389307369695,
            -6735444251963911948,
            -5262697557310666264,
        ];
        let actual = super::get_migrations()
            .iter()
            .map(|migration| migration.fingerprint() as i64)
            .collect::<Vec<_>>();

        assert_eq!(actual, expected);
    }

    #[tokio::test]
    async fn migrates_database_at_original_index_19() {
        let db = ConnectionPool::new("sqlite::memory:").unwrap();
        let migrations = super::get_migrations();
        db.initialize_database(migrations[..20].to_vec())
            .await
            .unwrap();

        super::migrate(&db).await.unwrap();

        let ledger = db
            .query("SELECT hash FROM __migrations WHERE namespace = '' AND id = 19")
            .await
            .unwrap();
        assert_eq!(
            ledger.rows().first().unwrap().get_int("hash"),
            Some(7684280566407992789)
        );
        db.query("SELECT agent_path FROM messages LIMIT 0")
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn full_name_migration_backfills_username() {
        let db = ConnectionPool::new("sqlite::memory:").unwrap();
        let migrations = super::get_migrations();
        db.initialize_database(migrations[..migrations.len() - 1].to_vec())
            .await
            .unwrap();
        db.query_with_params(
            "INSERT INTO users (id, username, password_hash, personality) VALUES (?, ?, ?, ?)",
            vec![
                Value::Uuid(uuid::Uuid::new_v4()),
                Value::Text("alice".to_string()),
                Value::Text("hash".to_string()),
                Value::Null,
            ],
        )
        .await
        .unwrap();

        db.initialize_database(migrations).await.unwrap();

        let rows = db
            .query("SELECT username, full_name FROM users")
            .await
            .unwrap();
        let row = rows.rows().first().unwrap();
        assert_eq!(row.get_text("username"), Some("alice"));
        assert_eq!(row.get_text("full_name"), Some("alice"));
    }

    #[tokio::test]
    async fn repairs_reordered_migrations_without_writable_dirs_migration() {
        repairs_reordered_migrations(false).await;
    }

    #[tokio::test]
    async fn repairs_reordered_migrations_with_writable_dirs_migration() {
        repairs_reordered_migrations(true).await;
    }

    async fn repairs_reordered_migrations(include_writable_dirs_migration: bool) {
        let db = ConnectionPool::new("sqlite::memory:").unwrap();
        let migrations = super::get_migrations();
        db.initialize_database(migrations[..19].to_vec())
            .await
            .unwrap();

        let owner = uuid::Uuid::new_v4();
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
        db.query_with_params(
            "INSERT INTO writable_dirs (id, owner, path, created_at) VALUES (?, ?, ?, ?)",
            vec![
                Value::Uuid(uuid::Uuid::new_v4()),
                Value::Uuid(owner),
                Value::Text("projects".to_string()),
                Value::Integer(0),
            ],
        )
        .await
        .unwrap();

        let mut broken = migrations[..19].to_vec();
        broken.extend_from_slice(&migrations[20..23]);
        if include_writable_dirs_migration {
            broken.push(migrations[19].clone());
        }
        db.initialize_database(broken).await.unwrap();

        super::migrate(&db).await.unwrap();

        let writable_dirs = db.query("SELECT path FROM writable_dirs").await.unwrap();
        assert_eq!(
            writable_dirs.rows().first().unwrap().get_text("path"),
            Some("/home/user/projects")
        );
        db.query("SELECT agent_path FROM messages LIMIT 0")
            .await
            .unwrap();
        db.query("SELECT agent_id FROM thread_agents LIMIT 0")
            .await
            .unwrap();
        let user = db.query("SELECT full_name FROM users").await.unwrap();
        assert_eq!(
            user.rows().first().unwrap().get_text("full_name"),
            Some("alice")
        );

        let ledger = db
            .query("SELECT id, hash FROM __migrations WHERE namespace = '' ORDER BY id")
            .await
            .unwrap();
        assert_eq!(ledger.rows().len(), migrations.len());
        for (index, row) in ledger.rows().iter().enumerate() {
            assert_eq!(row.get_int("id"), Some(index as i64));
            assert_eq!(
                row.get_int("hash").map(|hash| hash as u64),
                Some(migrations[index].fingerprint())
            );
        }
    }
}
