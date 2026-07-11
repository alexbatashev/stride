use std::sync::Arc;

use axum::{
    extract::State,
    http::HeaderMap,
    response::{Html, IntoResponse, Redirect, Response},
};

use crate::{ServerState, api::threads};

pub async fn files(State(state): State<Arc<ServerState>>, headers: HeaderMap) -> Response {
    match super::render_shell_page(state, headers, "files", "Files - S.T.R.I.D.E.").await {
        Ok(html) => Html(html).into_response(),
        Err(threads::ThreadApiError::Auth(_)) => Redirect::to("/auth/login").into_response(),
        Err(error) => error.into_response(),
    }
}
