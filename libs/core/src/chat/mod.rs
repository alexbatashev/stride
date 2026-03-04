mod model;
mod service;
mod storage;

pub use model::*;
pub use service::*;
pub use storage::*;

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

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

// #[allow(non_upper_case_globals)]
// mod chat_schema {
//     use super::*;

//     migrations! {
//         chat_schema {
//             table chat_threads {
//                 id: Uuid [PrimaryKey],
//                 user_id: Option<Uuid>,
//                 title: String,
//                 created_at: i64,
//                 updated_at: i64,
//                 preview_text: String,
//                 is_pinned: bool,
//             }

//             table chat_messages {
//                 id: Uuid [PrimaryKey],
//                 thread_id: Uuid,
//                 user_id: Option<Uuid>,
//                 parent_id: Option<Uuid>,
//                 provider_id: String,
//                 model_id: String,
//                 model_name: String,
//                 role: String,
//                 thinking: Option<String>,
//                 content: String,
//                 tool_call: Option<String>,
//                 tool_result: Option<String>,
//                 created_at: i64,
//                 updated_at: i64,
//                 is_done: bool,
//                 usage: Option<String>,

//                 foreign_key(thread_id -> chat_threads.id);
//             }
//         }
//     }
// }
