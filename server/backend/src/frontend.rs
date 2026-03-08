use std::collections::HashMap;
use std::io::{Cursor, Read};
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::{StatusCode, Uri, header};
use axum::response::Response;
use bytes::Bytes;

pub type Files = Arc<HashMap<String, Bytes>>;

static FRONTEND_TAR: &[u8] = include_bytes!(env!("FRONTEND_TAR"));

/// Parse the compile-time embedded tar archive into an in-memory file map.
///
/// Keys are archive-relative paths with no leading `./`
/// (e.g. `"index.html"`, `"bundle.js"`, `"proto/hello.proto"`).
pub fn init() -> Files {
    let mut archive = tar::Archive::new(Cursor::new(FRONTEND_TAR));
    let mut map = HashMap::new();

    for entry in archive.entries().expect("parse embedded frontend tar") {
        let mut entry = entry.expect("read tar entry");
        if entry.header().entry_type().is_dir() {
            continue;
        }
        let raw = entry.path().expect("entry path").to_string_lossy().into_owned();
        let key = raw.trim_start_matches("./").to_string();
        let mut content = Vec::new();
        entry.read_to_end(&mut content).expect("read tar entry content");
        map.insert(key, Bytes::from(content));
    }

    Arc::new(map)
}

/// Build an Axum router that serves all assets from the in-memory file map.
///
/// Unknown paths fall back to `index.html` (SPA-style routing).
pub fn http_router(files: Files) -> Router {
    Router::new()
        .fallback(serve_static)
        .with_state(files)
}

async fn serve_static(State(files): State<Files>, uri: Uri) -> Response<Body> {
    let raw = uri.path().trim_start_matches('/');
    let key = if raw.is_empty() { "index.html" } else { raw };

    let (data, mime) = match files.get(key) {
        Some(bytes) => (bytes.clone(), mime_for(key)),
        None => match files.get("index.html") {
            Some(bytes) => (bytes.clone(), "text/html; charset=utf-8"),
            None => {
                return Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(Body::empty())
                    .unwrap();
            }
        },
    };

    Response::builder()
        .header(header::CONTENT_TYPE, mime)
        .body(Body::from(data))
        .unwrap()
}

fn mime_for(path: &str) -> &'static str {
    match path.rsplit('.').next() {
        Some("html") => "text/html; charset=utf-8",
        Some("js") | Some("mjs") => "application/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json",
        Some("proto") => "text/plain; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        _ => "application/octet-stream",
    }
}
