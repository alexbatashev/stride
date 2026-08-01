use std::sync::Arc;

use axum::{
    extract::State,
    http::HeaderMap,
    response::{Html, IntoResponse, Redirect, Response},
};

use crate::{ServerState, api::threads};

pub async fn inbox(State(state): State<Arc<ServerState>>, headers: HeaderMap) -> Response {
    match super::render_shell_page(state, headers, "inbox", "Inbox - S.T.R.I.D.E.").await {
        Ok(html) => Html(html).into_response(),
        Err(threads::ThreadApiError::Auth(_)) => Redirect::to("/auth/login").into_response(),
        Err(_) => axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}
