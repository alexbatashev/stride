mod model;
mod service;
mod storage;

use std::time::{SystemTime, UNIX_EPOCH};

pub use model::*;
pub use service::*;
pub use storage::*;

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
