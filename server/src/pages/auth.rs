use std::sync::Arc;

use axum::{
    extract::State,
    response::{Html, IntoResponse, Response},
};
use serde_json::json;

use crate::ServerState;

pub async fn login(State(state): State<Arc<ServerState>>) -> Response {
    Html(super::render_page(
        &state.templates,
        "Log in",
        "",
        "auth",
        &json!({"mode": "login"}),
    ))
    .into_response()
}

pub async fn register(State(state): State<Arc<ServerState>>) -> Response {
    Html(super::render_page(
        &state.templates,
        "Register",
        "",
        "auth",
        &json!({"mode": "register"}),
    ))
    .into_response()
}
