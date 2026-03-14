mod model;
mod service;
mod storage;
mod tool_calls;
mod transport;

use std::time::{SystemTime, UNIX_EPOCH};

pub use model::*;
pub use service::*;
pub use storage::*;
pub use transport::*;

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
