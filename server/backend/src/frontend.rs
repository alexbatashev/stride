use axum::Router;
use tower_http::services::ServeDir;

pub(crate) fn http_router(static_dir: String, proto_dir: String) -> Router {
    Router::new()
        .nest_service("/proto", ServeDir::new(proto_dir))
        .fallback_service(ServeDir::new(static_dir))
}
