use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::HeaderMap,
    response::{Html, IntoResponse, Redirect, Response},
};
use uuid::Uuid;

use crate::{ServerState, api::threads};

pub(super) const PAGE_SCRIPT: &str =
    r#"<script type="module" src="/static/pages/threads-page.js"></script>"#;

pub async fn new_thread(State(state): State<Arc<ServerState>>, headers: HeaderMap) -> Response {
    render_threads(state, headers, None).await
}

pub async fn thread(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    render_threads(state, headers, Some(id)).await
}

async fn render_threads(
    state: Arc<ServerState>,
    headers: HeaderMap,
    thread_id: Option<Uuid>,
) -> Response {
    let data = match threads::thread_page_data(&state, &headers, thread_id).await {
        Ok(data) => data,
        Err(threads::ThreadApiError::Auth(_)) => {
            return Redirect::to("/auth/login").into_response();
        }
        Err(error) => return error.into_response(),
    };

    Html(super::render_threads_page(&data)).into_response()
}
