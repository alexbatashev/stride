#![allow(non_upper_case_globals)]

use minisql::{ConnectionPool, Migration, migrations};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::{Mutex, OnceCell};
use uuid::Uuid;

migrations! {
    chat_schema {
        table chat_threads {
            id: Uuid [PrimaryKey],
            user_id: Option<Uuid>,
            title: String,
            created_at: i64,
            updated_at: i64,
            preview_text: String,
            is_pinned: bool,
        }

        table chat_messages {
            id: Uuid [PrimaryKey],
            thread_id: Uuid,
            user_id: Option<Uuid>,
            parent_id: Option<Uuid>,
            provider_id: String,
            model_id: String,
            model_name: String,
            role: String,
            thinking: Option<String>,
            content: String,
            tool_call: Option<String>,
            tool_result: Option<String>,
            created_at: i64,
            updated_at: i64,
            is_done: bool,
            usage: Option<String>,

            foreign_key(thread_id -> chat_threads.id);
        }
    }
}
