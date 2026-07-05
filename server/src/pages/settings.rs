use std::sync::Arc;

use axum::{
    extract::State,
    http::HeaderMap,
    response::{Html, IntoResponse, Redirect, Response},
};

use crate::{ServerState, api::threads};

pub(super) fn page_script() -> String {
    super::module_script("pages/settings-page.js")
}

pub async fn settings(State(state): State<Arc<ServerState>>, headers: HeaderMap) -> Response {
    let data = match threads::thread_page_data(&state, &headers, None).await {
        Ok(data) => data,
        Err(threads::ThreadApiError::Auth(_)) => {
            return Redirect::to("/auth/login").into_response();
        }
        Err(_) => return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    Html(super::render_settings_page(&data)).into_response()
}
