use std::sync::Arc;

use axum::{
    extract::State,
    response::{Html, IntoResponse, Response},
};
use serde_json::json;

use crate::ServerState;

const PAGE_SCRIPT: &str = r#"<script type="module" src="/static/pages/sample-page.js"></script>"#;

pub async fn new_thread(State(state): State<Arc<ServerState>>) -> Response {
    Html(super::render_page(
        &state.templates,
        "Friday",
        PAGE_SCRIPT,
        "threads",
        &json!({}),
    ))
    .into_response()
}
