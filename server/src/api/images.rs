use std::sync::Arc;

use axum::{
    body::Body,
    extract::{Path, State},
    http::header,
    response::{IntoResponse, Response},
};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use llm::ImageSource;
use minisql::Value;
use uuid::Uuid;

use crate::ServerState;
use crate::db::public_images;
use crate::vfs::Vfs;

/// Turns raw image bytes into an [`ImageSource`] ready to send to a vision
/// model. When `public_url` is set the bytes are published behind an
/// unguessable capability URL served by [`serve`]; otherwise they are carried
/// inline as base64.
pub async fn publish_image(
    vfs: &Vfs,
    db: &minisql::ConnectionPool,
    owner: Uuid,
    public_url: Option<&str>,
    bytes: &[u8],
    mime_type: Option<&str>,
) -> anyhow::Result<ImageSource> {
    let mime = mime_type.unwrap_or("image/png");

    let Some(public_url) = public_url else {
        return Ok(ImageSource::base64(mime, STANDARD.encode(bytes)));
    };

    let location = vfs.store_blob(bytes).await?;
    let token = Uuid::now_v7().as_simple().to_string();
    let created_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default();

    public_images::insert()
        .id(Uuid::now_v7())
        .token(token.as_str())
        .owner(owner)
        .location(location.as_str())
        .mime_type(Some(mime))
        .created_at(created_at)
        .execute(db)
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;

    Ok(ImageSource::url(format!(
        "{public_url}/api/public/images/{token}"
    )))
}

/// Serves a published image by its capability token. Intentionally
/// unauthenticated so external model providers can fetch the URL; the token is
/// the only access control.
pub async fn serve(State(state): State<Arc<ServerState>>, Path(token): Path<String>) -> Response {
    let Some(ref vfs) = state.vfs else {
        return (axum::http::StatusCode::NOT_FOUND, "not found").into_response();
    };

    let row = state
        .db
        .query_with_params(
            "SELECT location, mime_type FROM public_images WHERE token = ? LIMIT 1",
            vec![Value::Text(token)],
        )
        .await;

    let Ok(result) = row else {
        return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "error").into_response();
    };
    let Some(row) = result.rows().first() else {
        return (axum::http::StatusCode::NOT_FOUND, "not found").into_response();
    };
    let Some(location) = row.get_text("location") else {
        return (axum::http::StatusCode::NOT_FOUND, "not found").into_response();
    };
    let content_type = row
        .get_text("mime_type")
        .unwrap_or("application/octet-stream")
        .to_string();

    match vfs.load_blob(location).await {
        Ok(bytes) => Response::builder()
            .header(header::CONTENT_TYPE, content_type)
            .header(header::CACHE_CONTROL, "private, max-age=86400")
            .body(Body::from(bytes))
            .unwrap_or_else(|_| {
                (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "error").into_response()
            }),
        Err(_) => (axum::http::StatusCode::NOT_FOUND, "not found").into_response(),
    }
}
