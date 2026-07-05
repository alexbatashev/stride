use std::sync::Arc;

use axum::{
    extract::State,
    http::HeaderMap,
    response::{Html, IntoResponse, Redirect, Response},
};

use crate::{ServerState, api::threads};

pub(super) fn page_script() -> String {
    super::module_script("pages/files-page.js")
}

pub async fn files(State(state): State<Arc<ServerState>>, headers: HeaderMap) -> Response {
    let data = match threads::thread_page_data(&state, &headers, None).await {
        Ok(data) => data,
        Err(threads::ThreadApiError::Auth(_)) => {
            return Redirect::to("/auth/login").into_response();
        }
        Err(error) => return error.into_response(),
    };

    Html(super::render_files_page(&data)).into_response()
}
