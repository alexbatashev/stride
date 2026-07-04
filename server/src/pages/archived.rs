use std::sync::Arc;

use axum::{
    extract::State,
    http::HeaderMap,
    response::{Html, IntoResponse, Redirect, Response},
};

use crate::{ServerState, api::threads};

pub const PAGE_SCRIPT: &str =
    r#"<script type="module" src="/static/pages/archived-page.js"></script>"#;

pub async fn archived(State(state): State<Arc<ServerState>>, headers: HeaderMap) -> Response {
    let data = match threads::thread_page_data(&state, &headers, None).await {
        Ok(data) => data,
        Err(threads::ThreadApiError::Auth(_)) => {
            return Redirect::to("/auth/login").into_response();
        }
        Err(_) => return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    Html(super::render_archived_page(&data)).into_response()
}
