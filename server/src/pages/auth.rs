use axum::response::{Html, IntoResponse, Response};

pub async fn login() -> Response {
    Html(super::render_auth_page("login")).into_response()
}

pub async fn register() -> Response {
    Html(super::render_auth_page("register")).into_response()
}
