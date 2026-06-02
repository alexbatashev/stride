use std::sync::Arc;

use axum::{
    extract::State,
    http::HeaderMap,
    response::{Html, IntoResponse, Redirect, Response},
};

use crate::{ServerState, api::threads};

const PAGE_SCRIPT: &str = r#"<script type="module" src="/static/pages/files-page.js"></script>"#;

pub async fn files(State(state): State<Arc<ServerState>>, headers: HeaderMap) -> Response {
    let data = match threads::thread_page_data(&state, &headers, None).await {
        Ok(data) => data,
        Err(threads::ThreadApiError::Auth(_)) => {
            return Redirect::to("/auth/login").into_response();
        }
        Err(error) => return error.into_response(),
    };

    let mut value = serde_json::to_value(data).unwrap();
    value["files_active"] = serde_json::Value::Bool(true);

    Html(super::render_page(
        &state.templates,
        "Files - Friday",
        PAGE_SCRIPT,
        "files",
        &value,
    ))
    .into_response()
}
