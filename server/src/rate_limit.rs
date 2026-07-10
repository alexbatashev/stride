use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::{
    extract::{ConnectInfo, Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use stride_agent::{Clock, SystemClock};

const MAX_REQUESTS: u32 = 10;
const WINDOW: Duration = Duration::from_secs(60);
const FALLBACK_KEY: &str = "unknown";

struct Window {
    count: u32,
    started: Instant,
}

pub struct RateLimiter {
    windows: Mutex<HashMap<String, Window>>,
    clock: Arc<dyn Clock>,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self::with_clock(Arc::new(SystemClock))
    }

    pub fn with_clock(clock: Arc<dyn Clock>) -> Self {
        Self {
            windows: Mutex::new(HashMap::new()),
            clock,
        }
    }

    fn check(&self, key: &str) -> bool {
        let now = self.clock.now_instant();
        let mut windows = self.windows.lock().unwrap();
        let window = windows.entry(key.to_string()).or_insert(Window {
            count: 0,
            started: now,
        });

        if now.duration_since(window.started) >= WINDOW {
            window.count = 0;
            window.started = now;
        }

        if window.count >= MAX_REQUESTS {
            return false;
        }

        window.count += 1;
        true
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

fn client_key(request: &Request) -> String {
    if let Some(value) = request.headers().get("x-forwarded-for")
        && let Ok(text) = value.to_str()
        && let Some(first) = text.split(',').next()
    {
        let trimmed = first.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }

    if let Some(ConnectInfo(addr)) = request.extensions().get::<ConnectInfo<SocketAddr>>() {
        return addr.ip().to_string();
    }

    FALLBACK_KEY.to_string()
}

pub async fn limit(
    State(limiter): State<Arc<RateLimiter>>,
    request: Request,
    next: Next,
) -> Response {
    let key = client_key(&request);
    if !limiter.check(&key) {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    }
    next.run(request).await
}
