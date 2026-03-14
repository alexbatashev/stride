use super::ChatMessage;
use super::TurnRole;
use super::now_millis;

use std::sync::Arc;

use async_lock::{Mutex, OnceCell};
use futures::future::BoxFuture;
use minisql::ConnectionPool;
use uuid::Uuid;

pub trait ChatStorage: Send + Sync {
    fn list_messages<'a>(&'a self) -> BoxFuture<'a, Vec<ChatMessage>>;
    fn append_message<'a>(&'a self, message: ChatMessage) -> BoxFuture<'a, ()>;
}

#[derive(Debug, Default)]
pub struct NullChatStorage;

impl ChatStorage for NullChatStorage {
    fn list_messages<'a>(&'a self) -> BoxFuture<'a, Vec<ChatMessage>> {
        Box::pin(async { vec![] })
    }

    fn append_message<'a>(&'a self, _message: ChatMessage) -> BoxFuture<'a, ()> {
        Box::pin(async {})
    }
}

#[derive(Debug, Default)]
pub struct MockChatStorage {
    messages: Mutex<Vec<ChatMessage>>,
}

impl MockChatStorage {
    pub fn new(messages: Vec<ChatMessage>) -> Self {
        let mut sorted = messages;
        sorted.sort_by_key(|m| m.created_at_ms);
        Self {
            messages: Mutex::new(sorted),
        }
    }
}

impl ChatStorage for MockChatStorage {
    fn list_messages<'a>(&'a self) -> BoxFuture<'a, Vec<ChatMessage>> {
        Box::pin(async move { self.messages.lock().await.clone() })
    }

    fn append_message<'a>(&'a self, message: ChatMessage) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            self.messages.lock().await.push(message);
        })
    }
}

pub struct LocalChatStorage {
    chat_thread_id: Uuid,
    database: Arc<ConnectionPool>,
    migrations_ready: OnceCell<()>,
}

impl LocalChatStorage {
    pub fn new(thread_id: Uuid, database: Arc<ConnectionPool>) -> Self {
        Self {
            chat_thread_id: thread_id,
            database,
            migrations_ready: OnceCell::new(),
        }
    }

    async fn ensure_ready(&self) {
        let _ = self
            .migrations_ready
            .get_or_init(|| async {
                let _ = self
                    .database
                    .initialize_database(crate::data::get_migrations())
                    .await;
            })
            .await;
    }
}

impl ChatStorage for LocalChatStorage {
    fn list_messages<'a>(&'a self) -> BoxFuture<'a, Vec<ChatMessage>> {
        Box::pin(async move {
            self.ensure_ready().await;
            let result = crate::data::chat_messages::select()
                .where_(crate::data::chat_messages::thread_id.eq(self.chat_thread_id))
                .order_by_asc(crate::data::chat_messages::created_at)
                .all(&*self.database)
                .await;

            match result {
                Ok(rows) => rows
                    .into_iter()
                    .filter_map(|row| {
                        let role = match row.role.as_str() {
                            "User" => TurnRole::User,
                            "Assistant" => TurnRole::Assistant,
                            "Tool" => TurnRole::Tool,
                            "System" => TurnRole::System,
                            _ => return None,
                        };
                        Some(ChatMessage {
                            id: row.id,
                            thread_id: row.thread_id,
                            user_id: row.user_id,
                            parent_id: row.parent_id,
                            provider_id: row.provider_id,
                            model_id: row.model_id,
                            model_name: row.model_name,
                            role,
                            thinking: row.thinking,
                            content: row.content,
                            tool_call: row.tool_call,
                            tool_result: row.tool_result,
                            created_at_ms: row.created_at,
                            updated_at_ms: row.updated_at,
                            is_done: row.is_done,
                            usage: row.usage,
                        })
                    })
                    .collect(),
                Err(_) => vec![],
            }
        })
    }

    fn append_message<'a>(&'a self, message: ChatMessage) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            self.ensure_ready().await;
            let now = now_millis();
            let preview = message.content.trim().to_owned();
            let existing = crate::data::chat_threads::select()
                .where_(crate::data::chat_threads::id.eq(self.chat_thread_id))
                .limit(1)
                .all(&*self.database)
                .await;

            if let Ok(mut existing) = existing {
                if let Some(thread) = existing.pop() {
                    let updated_at = thread.updated_at.max(message.updated_at_ms);
                    let title = if thread.title.is_empty() {
                        if preview.is_empty() {
                            "Chat".to_owned()
                        } else {
                            preview.chars().take(80).collect::<String>()
                        }
                    } else {
                        thread.title
                    };
                    let next_preview = if preview.is_empty() {
                        thread.preview_text
                    } else {
                        preview.clone()
                    };
                    let _ = self
                        .database
                        .query_with_params(
                            "UPDATE chat_threads SET title = ?, updated_at = ?, preview_text = ? WHERE id = ?",
                            vec![
                                minisql::Value::Text(title),
                                minisql::Value::Integer(updated_at),
                                minisql::Value::Text(next_preview),
                                minisql::Value::Uuid(self.chat_thread_id),
                            ],
                        )
                        .await;
                } else {
                    let _ = crate::data::chat_threads::insert()
                        .id(self.chat_thread_id)
                        .user_id(message.user_id)
                        .title(if preview.is_empty() {
                            "Chat".to_owned()
                        } else {
                            preview.chars().take(80).collect::<String>()
                        })
                        .created_at(message.created_at_ms)
                        .updated_at(message.updated_at_ms)
                        .preview_text(preview.clone())
                        .is_pinned(false)
                        .execute(&*self.database)
                        .await;
                }
            }

            let role_str = match message.role {
                TurnRole::User => "User",
                TurnRole::Assistant => "Assistant",
                TurnRole::Tool => "Tool",
                TurnRole::System => "System",
            };
            let _ = crate::data::chat_messages::insert()
                .id(message.id)
                .thread_id(message.thread_id)
                .user_id(message.user_id)
                .parent_id(message.parent_id)
                .provider_id(message.provider_id)
                .model_id(message.model_id)
                .model_name(message.model_name)
                .role(role_str)
                .thinking(message.thinking)
                .content(message.content)
                .tool_call(message.tool_call)
                .tool_result(message.tool_result)
                .created_at(message.created_at_ms)
                .updated_at(message.updated_at_ms)
                .is_done(message.is_done)
                .usage(message.usage)
                .execute(&*self.database)
                .await;
        })
    }
}
