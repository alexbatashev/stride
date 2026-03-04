use super::ChatMessage;
use super::now_millis;

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_stream::stream;
use futures::{Stream, StreamExt, future::BoxFuture};
use llm::{
    API, Completion, CompletionChoice, CompletionRequest, Message, OpenAI, Role, UnnamedToolChoice,
};
use minisql::{ConnectionPool, Migration, migrations};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::{Mutex, OnceCell};
use uuid::Uuid;

use crate::tools::{Tool, ToolArg};

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
        sorted.sort_by_key(|m| m.created_at);
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
    database: ConnectionPool,
    migrations_ready: OnceCell<()>,
}

impl LocalChatStorage {
    pub fn new(thread_id: Uuid, database: ConnectionPool) -> Self {
        Self {
            chat_thread_id: thread_id,
            database,
            migrations_ready: OnceCell::new(),
        }
    }
}

impl ChatStorage for LocalChatStorage {
    fn list_messages<'a>(&'a self) -> BoxFuture<'a, Vec<ChatMessage>> {
        Box::pin(async move {
            let result = chat_schema::chat_messages::select()
                .where_(chat_schema::chat_messages::thread_id.eq(self.chat_thread_id))
                .order_by_asc(chat_schema::chat_messages::created_at)
                .all(&self.database)
                .await;

            match result {
                Ok(rows) => todo!(), // rows.into_iter().map(StoredChatMessage::from_row).collect(),
                Err(_) => vec![],
            }
        })
    }

    fn append_message<'a>(&'a self, message: ChatMessage) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            let now = now_millis();
            let preview = message.content.trim().to_owned();
            let existing = chat_schema::chat_threads::select()
                .where_(chat_schema::chat_threads::id.eq(self.chat_thread_id))
                .limit(1)
                .all(&self.database)
                .await;

            if let Ok(mut existing) = existing {
                if let Some(thread) = existing.pop() {
                    let updated_at = thread.updated_at.max(message.updated_at);
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
                    // let _ = chat_schema::chat_threads::insert()
                    //     .id(self.chat_thread_id)
                    //     .user_id(message.user_id)
                    //     .title(if preview.is_empty() {
                    //         "Chat".to_owned()
                    //     } else {
                    //         preview.chars().take(80).collect::<String>()
                    //     })
                    //     .created_at(message.created_at)
                    //     .updated_at(message.updated_at)
                    //     .preview_text(preview.clone())
                    //     .is_pinned(false)
                    //     .execute(&self.database)
                    //     .await;
                }
            }

            // let stored = StoredChatMessage::from_domain(self.chat_thread_id, message, now);
            // let _ = chat_schema::chat_messages::insert()
            //     .id(stored.id)
            //     .thread_id(stored.thread_id)
            //     .user_id(stored.user_id)
            //     .parent_id(stored.parent_id)
            //     .provider_id(stored.provider_id)
            //     .model_id(stored.model_id)
            //     .model_name(stored.model_name)
            //     .role(stored.role)
            //     .thinking(stored.thinking)
            //     .content(stored.content)
            //     .tool_call(stored.tool_call)
            //     .tool_result(stored.tool_result)
            //     .created_at(stored.created_at)
            //     .updated_at(stored.updated_at)
            //     .is_done(stored.is_done)
            //     .usage(stored.usage)
            //     .execute(&self.database)
            //     .await;
        })
    }
}
