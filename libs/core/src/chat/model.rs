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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, uniffi::Enum)]
pub enum TurnRole {
    User,
    Assistant,
    Tool,
    System,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ToolInvocationStatus {
    Queued,
    Running,
    Completed,
    Failed,
}

impl ToolInvocationStatus {
    fn as_str(self) -> &'static str {
        match self {
            ToolInvocationStatus::Queued => "queued",
            ToolInvocationStatus::Running => "running",
            ToolInvocationStatus::Completed => "completed",
            ToolInvocationStatus::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInvocation {
    pub id: Uuid,
    pub name: String,
    pub arguments_json: String,
    pub result_json: Option<String>,
    pub status: ToolInvocationStatus,
    pub started_at: i64,
    pub ended_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatThread {
    pub id: Uuid,
    pub user_id: Option<Uuid>,
    pub title: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub preview_text: String,
    pub is_pinned: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, uniffi::Record)]
pub struct ChatMessage {
    pub id: Uuid,
    pub thread_id: Uuid,
    pub user_id: Option<Uuid>,
    pub parent_id: Option<Uuid>,
    pub provider_id: String,
    pub model_id: String,
    pub model_name: String,
    pub role: TurnRole,
    pub thinking: Option<String>,
    pub content: String,
    pub tool_call: Option<String>,
    pub tool_result: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub is_done: bool,
    pub usage: Option<String>,
}

impl ChatMessage {
    pub fn new(role: TurnRole, content: impl Into<String>) -> Self {
        let now = now_millis();
        Self {
            id: Uuid::new_v4(),
            thread_id: Uuid::new_v4(),
            user_id: None,
            parent_id: None,
            provider_id: String::new(),
            model_id: String::new(),
            model_name: String::new(),
            role,
            thinking: None,
            content: content.into(),
            tool_call: None,
            tool_result: None,
            created_at: now,
            updated_at: now,
            is_done: false,
            usage: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, uniffi::Record)]
pub struct LangModel {
    pub provider: String,
    pub model: String,
    pub provider_name: String,
    pub model_name: String,
}
