use std::sync::Arc;

use axum::{
    extract::{Path, State},
    response::{Html, IntoResponse, Response},
};
use serde_json::json;

use crate::ServerState;

const PAGE_SCRIPT: &str = r#"<script type="module" src="/static/pages/threads-page.js"></script>"#;

pub async fn new_thread(State(state): State<Arc<ServerState>>) -> Response {
    render_threads(state, None)
}

pub async fn thread(State(state): State<Arc<ServerState>>, Path(id): Path<String>) -> Response {
    render_threads(state, Some(id))
}

fn render_threads(state: Arc<ServerState>, thread_id: Option<String>) -> Response {
    Html(super::render_page(
        &state.templates,
        "Friday",
        PAGE_SCRIPT,
        "threads",
        &json!({ "thread_id": thread_id }),
    ))
    .into_response()
}
