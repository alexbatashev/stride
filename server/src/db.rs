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
    Webhook,
    Manual,
    VfsChange,
}

impl TriggerKind {
    pub fn as_str(self) -> &'static str {
        match self {
            TriggerKind::Cron => "cron",
            TriggerKind::Email => "email",
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
            name: String [Unique],
            title: String,
            description: String,
            content: String,
            owner: Option<Uuid>,

            foreign_key(owner -> users.id);
        }

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

    // Memory palace: a per-user, spatially organized long-term memory inspired by
    // MemPalace. Wings hold rooms, rooms hold drawers (verbatim originals) indexed
    // by closets (compressed summary cards carrying the vector embedding). Doors
    // link rooms so a memory can be reached from many angles.
    memory_palace {
        table memory_wings {
            id: Uuid [PrimaryKey],
            owner: Uuid,
            name: String,
            description: String,
            created_at: i64,

            foreign_key(owner -> users.id);
        }

        table memory_rooms {
            id: Uuid [PrimaryKey],
            owner: Uuid,
            wing: Uuid,
            name: String,
            description: String,
            created_at: i64,

            foreign_key(owner -> users.id);
            foreign_key(wing -> memory_wings.id);
        }

        table memory_halls {
            id: Uuid [PrimaryKey],
            owner: Uuid,
            wing: Uuid,
            name: String,
            description: String,
            created_at: i64,

            foreign_key(owner -> users.id);
            foreign_key(wing -> memory_wings.id);
        }

        table memory_drawers {
            id: Uuid [PrimaryKey],
            owner: Uuid,
            room: Uuid,
            title: String,
            content: String,
            source: Option<String>,
            created_at: i64,

            foreign_key(owner -> users.id);
            foreign_key(room -> memory_rooms.id);
        }

        table memory_closets {
            id: Uuid [PrimaryKey],
            owner: Uuid,
            room: Uuid,
            drawer: Uuid,
            hall: Option<Uuid>,
            summary: String,
            keywords: String,
            embedding: Option<Embedding>,
            created_at: i64,

            foreign_key(owner -> users.id);
            foreign_key(room -> memory_rooms.id);
            foreign_key(drawer -> memory_drawers.id);
            foreign_key(hall -> memory_halls.id);
        }

        table memory_doors {
            id: Uuid [PrimaryKey],
            owner: Uuid,
            from_room: Uuid,
            to_room: Uuid,
            relation: String,
            created_at: i64,

            foreign_key(owner -> users.id);
            foreign_key(from_room -> memory_rooms.id);
            foreign_key(to_room -> memory_rooms.id);
        }

        raw "CREATE UNIQUE INDEX IF NOT EXISTS idx_memory_wings_owner_name ON memory_wings(owner, name)";
        raw "CREATE UNIQUE INDEX IF NOT EXISTS idx_memory_rooms_wing_name ON memory_rooms(wing, name)";
        raw "CREATE INDEX IF NOT EXISTS idx_memory_closets_owner ON memory_closets(owner)";
        raw "CREATE INDEX IF NOT EXISTS idx_memory_drawers_room ON memory_drawers(room)";
    }
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

/// A vector embedding stored as a raw little-endian `f32` blob. Kept as an
/// opaque byte column so the schema is independent of the embedding model's
/// dimension; cosine distance is computed by sqlite-vec at query time.
#[derive(Clone, Debug)]
pub struct Embedding(pub Vec<u8>);

impl Embedding {
    /// Pack a float vector into the little-endian byte layout sqlite-vec reads.
    pub fn from_floats(values: &[f32]) -> Self {
        let mut bytes = Vec::with_capacity(values.len() * 4);
        for v in values {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        Embedding(bytes)
    }
}

impl SqlLikeType for Embedding {
    fn as_sql_type() -> minisql::SqlType {
        minisql::SqlType::Blob
    }
}

impl FromValue for Embedding {
    fn from_value(v: &Value) -> Result<Self, DecodeError> {
        match v {
            Value::Blob(b) => Ok(Embedding(b.clone())),
            other => Err(DecodeError(format!(
                "expected BLOB for Embedding, got {other}"
            ))),
        }
    }
}

impl From<Embedding> for Value {
    fn from(val: Embedding) -> Value {
        Value::Blob(val.0)
    }
}

impl IntoValue for Embedding {
    fn into_value(self) -> Value {
        Value::Blob(self.0)
    }
}
