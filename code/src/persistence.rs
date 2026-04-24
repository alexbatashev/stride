#![allow(non_upper_case_globals)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow};
use llm::{Message, Role};
use minisql::{ConnectionPool, Value, migrations};
use uuid::Uuid;

migrations! {
    schema {
        table threads {
            id: String [PrimaryKey],
            cwd: String,
            created_at: i64,
            updated_at: i64,
            preview: Option<String>,
        }

        table messages {
            id: String [PrimaryKey],
            thread_id: String,
            seq: i64,
            role: String,
            content: String,
            thinking: Option<String>,
            tool_call_id: Option<String>,
            created_at: i64,

            foreign_key(thread_id -> threads.id);
        }

        raw "CREATE INDEX idx_threads_cwd_updated_at ON threads(cwd, updated_at DESC);";
        raw "CREATE INDEX idx_messages_thread_seq ON messages(thread_id, seq ASC);";
    }
}

#[derive(Clone)]
pub struct ThreadStore {
    db: ConnectionPool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadSummary {
    pub id: String,
    pub cwd: String,
    pub updated_at: i64,
    pub preview: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryMessage {
    pub seq: u32,
    pub role: String,
    pub content: String,
    pub thinking: String,
    pub tool_call_id: String,
    pub tool_name: String,
    pub created_at: i64,
}

impl ThreadStore {
    pub async fn new(path: PathBuf) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }

        let url = format!("sqlite:{}", path.display());
        let db = ConnectionPool::new(&url).map_err(sql_error)?;
        db.initialize_database(get_migrations())
            .await
            .map_err(sql_error)?;
        ensure_runtime_schema(&db).await?;
        Ok(Self { db })
    }

    pub async fn create_thread(&self, cwd: &Path, messages: &[Message]) -> Result<String> {
        let thread_id = Uuid::new_v4().to_string();
        self.save_thread(&thread_id, cwd, messages, &HashMap::new())
            .await?;
        Ok(thread_id)
    }

    pub async fn save_thread(
        &self,
        thread_id: &str,
        cwd: &Path,
        messages: &[Message],
        tool_names: &HashMap<usize, String>,
    ) -> Result<()> {
        let cwd = cwd_to_string(cwd);
        let now = now_ts();
        let preview = preview_text(messages);
        let tx = self.db.transaction().await.map_err(sql_error)?;

        self.db
            .query_with_params(
                "INSERT OR REPLACE INTO threads (id, cwd, created_at, updated_at, preview)
                 VALUES (
                   ?,
                   ?,
                   COALESCE((SELECT created_at FROM threads WHERE id = ?), ?),
                   ?,
                   ?
                 )",
                vec![
                    Value::Text(thread_id.to_string()),
                    Value::Text(cwd.clone()),
                    Value::Text(thread_id.to_string()),
                    Value::Integer(now),
                    Value::Integer(now),
                    optional_text(preview),
                ],
            )
            .await
            .map_err(sql_error)?;

        self.db
            .query_with_params(
                "DELETE FROM messages WHERE thread_id = ?",
                vec![Value::Text(thread_id.to_string())],
            )
            .await
            .map_err(sql_error)?;

        for (seq, message) in messages.iter().enumerate() {
            self.db
                .query_with_params(
                    "INSERT INTO messages (id, thread_id, seq, role, content, thinking, tool_call_id, tool_name, created_at)
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                    vec![
                        Value::Text(Uuid::new_v4().to_string()),
                        Value::Text(thread_id.to_string()),
                        Value::Integer(seq as i64),
                        Value::Text(role_to_string(&message.role).to_string()),
                        Value::Text(message.content.clone()),
                        optional_text(message.thinking.clone()),
                        optional_text(message.tool_call_id.clone()),
                        optional_text(tool_names.get(&seq).cloned()),
                        Value::Integer(now),
                    ],
                )
                .await
                .map_err(sql_error)?;
        }

        tx.commit().await.map_err(sql_error)?;
        Ok(())
    }

    pub async fn load_thread(&self, thread_id: &str) -> Result<Option<(PathBuf, Vec<Message>)>> {
        let thread = threads::select()
            .where_(threads::id.eq(thread_id))
            .one(&self.db)
            .await;

        let thread = match thread {
            Ok(row) => row,
            Err(_) => return Ok(None),
        };

        let message_rows = messages::select()
            .where_(messages::thread_id.eq(thread_id))
            .order_by_asc(messages::seq)
            .all(&self.db)
            .await
            .map_err(sql_error)?;

        let mut out = Vec::with_capacity(message_rows.len());
        for row in message_rows {
            out.push(Message {
                role: parse_role(&row.role)?,
                content: row.content,
                thinking: row.thinking,
                tool_calls: None,
                tool_call_id: row.tool_call_id,
            });
        }

        Ok(Some((PathBuf::from(thread.cwd), out)))
    }

    pub async fn latest_thread_for_cwd(&self, cwd: &Path) -> Result<Option<String>> {
        let cwd = cwd_to_string(cwd);
        let rows = threads::select_cols((threads::id,))
            .where_(threads::cwd.eq(cwd.as_str()))
            .order_by_desc(threads::updated_at)
            .limit(1)
            .all(&self.db)
            .await
            .map_err(sql_error)?;

        Ok(rows.into_iter().next().map(|(id,)| id))
    }

    pub async fn list_threads(&self, cwd: &Path, limit: u32) -> Result<Vec<ThreadSummary>> {
        let cwd = cwd_to_string(cwd);
        let rows = threads::select()
            .where_(threads::cwd.eq(cwd.as_str()))
            .order_by_desc(threads::updated_at)
            .limit(limit as u64)
            .all(&self.db)
            .await
            .map_err(sql_error)?;

        Ok(rows
            .into_iter()
            .map(|row| ThreadSummary {
                id: row.id,
                cwd: row.cwd,
                updated_at: row.updated_at,
                preview: row.preview.unwrap_or_default(),
            })
            .collect())
    }

    pub async fn history(&self, thread_id: &str) -> Result<Vec<HistoryMessage>> {
        let rows = self
            .db
            .query_with_params(
                "SELECT seq, role, content, thinking, tool_call_id, tool_name, created_at
                 FROM messages
                 WHERE thread_id = ?
                 ORDER BY seq ASC",
                vec![Value::Text(thread_id.to_string())],
            )
            .await
            .map_err(sql_error)?;

        Ok(rows
            .rows()
            .iter()
            .map(|row| HistoryMessage {
                seq: row.get_int("seq").unwrap_or_default() as u32,
                role: row.get_text("role").unwrap_or_default().to_string(),
                content: row.get_text("content").unwrap_or_default().to_string(),
                thinking: row.get_text("thinking").unwrap_or_default().to_string(),
                tool_call_id: row.get_text("tool_call_id").unwrap_or_default().to_string(),
                tool_name: row.get_text("tool_name").unwrap_or_default().to_string(),
                created_at: row.get_int("created_at").unwrap_or_default(),
            })
            .collect())
    }
}

fn sql_error(error: Box<dyn std::error::Error + Send + Sync>) -> anyhow::Error {
    anyhow!(error.to_string())
}

pub fn default_database_path(config_path: &Path, configured: Option<&str>) -> PathBuf {
    configured
        .map(expand_path)
        .unwrap_or_else(|| default_state_dir(config_path).join("threads.sqlite"))
}

pub fn default_socket_path(config_path: &Path, configured: Option<&str>) -> PathBuf {
    configured
        .map(expand_path)
        .unwrap_or_else(|| runtime_dir(config_path).join("friday.sock"))
}

fn cwd_to_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn role_to_string(role: &Role) -> &'static str {
    match role {
        Role::System => "system",
        Role::Assistant => "assistant",
        Role::User => "user",
        Role::Tool => "tool",
    }
}

fn parse_role(value: &str) -> Result<Role> {
    match value {
        "system" => Ok(Role::System),
        "assistant" => Ok(Role::Assistant),
        "user" => Ok(Role::User),
        "tool" => Ok(Role::Tool),
        other => Err(anyhow!("unknown message role '{}'", other)),
    }
}

fn preview_text(messages: &[Message]) -> Option<String> {
    messages
        .iter()
        .rev()
        .find_map(|message| match message.role {
            Role::User | Role::Assistant => {
                let trimmed = message.content.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(truncate_preview(trimmed))
                }
            }
            _ => None,
        })
}

fn truncate_preview(text: &str) -> String {
    const MAX_LEN: usize = 80;
    if text.chars().count() <= MAX_LEN {
        return text.to_string();
    }
    text.chars().take(MAX_LEN).collect::<String>() + "..."
}

fn optional_text(value: Option<String>) -> Value {
    match value {
        Some(value) => Value::Text(value),
        None => Value::Null,
    }
}

fn now_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as i64
}

async fn ensure_runtime_schema(db: &ConnectionPool) -> Result<()> {
    let columns = db
        .query("PRAGMA table_info(messages)")
        .await
        .map_err(sql_error)?;

    let has_tool_name = columns
        .rows()
        .iter()
        .any(|row| row.get_text("name") == Some("tool_name"));

    if !has_tool_name {
        db.query("ALTER TABLE messages ADD COLUMN tool_name TEXT")
            .await
            .map_err(sql_error)?;
    }

    Ok(())
}

fn default_state_dir(config_path: &Path) -> PathBuf {
    if let Ok(state_dir) = std::env::var("XDG_STATE_HOME") {
        return PathBuf::from(state_dir).join("friday");
    }

    if let Some(home) = dirs::home_dir() {
        return home.join(".local").join("state").join("friday");
    }

    config_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(".friday")
}

fn runtime_dir(config_path: &Path) -> PathBuf {
    if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
        return PathBuf::from(runtime_dir).join("friday");
    }

    default_state_dir(config_path)
}

fn expand_path(path: &str) -> PathBuf {
    if path == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from(path));
    }

    if let Some(stripped) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(stripped);
        }
    }

    PathBuf::from(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("friday-{}-{}.sqlite", name, Uuid::new_v4()))
    }

    fn sample_messages() -> Vec<Message> {
        vec![
            Message {
                role: Role::System,
                content: "system".to_string(),
                thinking: None,
                tool_calls: None,
                tool_call_id: None,
            },
            Message {
                role: Role::User,
                content: "hello".to_string(),
                thinking: None,
                tool_calls: None,
                tool_call_id: None,
            },
            Message {
                role: Role::Assistant,
                content: "world".to_string(),
                thinking: Some("thought".to_string()),
                tool_calls: None,
                tool_call_id: None,
            },
        ]
    }

    #[tokio::test]
    async fn save_and_resume_thread() {
        let path = temp_db_path("resume");
        let store = ThreadStore::new(path.clone()).await.unwrap();
        let cwd = PathBuf::from("/tmp/project-a");

        let thread_id = store.create_thread(&cwd, &sample_messages()).await.unwrap();
        let loaded = store.load_thread(&thread_id).await.unwrap().unwrap();

        assert_eq!(loaded.0, cwd);
        assert_eq!(loaded.1.len(), 3);
        assert_eq!(loaded.1[2].thinking.as_deref(), Some("thought"));

        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn list_and_find_latest_by_cwd() {
        let path = temp_db_path("list");
        let store = ThreadStore::new(path.clone()).await.unwrap();
        let cwd = PathBuf::from("/tmp/project-b");

        let first = store.create_thread(&cwd, &sample_messages()).await.unwrap();
        let mut updated_messages = sample_messages();
        updated_messages.push(Message {
            role: Role::User,
            content: "newer".to_string(),
            thinking: None,
            tool_calls: None,
            tool_call_id: None,
        });
        let second = store.create_thread(&cwd, &updated_messages).await.unwrap();

        let latest = store.latest_thread_for_cwd(&cwd).await.unwrap();
        let threads = store.list_threads(&cwd, 10).await.unwrap();

        assert_eq!(latest.as_deref(), Some(second.as_str()));
        assert_eq!(threads.len(), 2);
        assert!(threads.iter().any(|thread| thread.id == first));
        assert_eq!(threads[0].preview, "newer");

        let _ = std::fs::remove_file(path);
    }
}
