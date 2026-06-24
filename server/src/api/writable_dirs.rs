use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use minisql::ConnectionPool;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::{
    ServerState,
    api::auth::{self, AuthError},
    db::writable_dirs,
    vfs::WORKSPACE_MOUNT,
};

#[derive(Debug)]
pub enum WritableDirApiError {
    Auth(AuthError),
    BadRequest(String),
    Conflict,
    NotFound,
    Internal,
}

impl IntoResponse for WritableDirApiError {
    fn into_response(self) -> Response {
        match self {
            WritableDirApiError::Auth(error) => error.into_response(),
            WritableDirApiError::BadRequest(message) => {
                (StatusCode::BAD_REQUEST, Json(json!({"error": message}))).into_response()
            }
            WritableDirApiError::Conflict => (
                StatusCode::CONFLICT,
                Json(json!({"error": "this directory is already writable"})),
            )
                .into_response(),
            WritableDirApiError::NotFound => (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "writable directory not found"})),
            )
                .into_response(),
            WritableDirApiError::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "internal server error"})),
            )
                .into_response(),
        }
    }
}

impl From<AuthError> for WritableDirApiError {
    fn from(error: AuthError) -> Self {
        WritableDirApiError::Auth(error)
    }
}

#[derive(Serialize)]
pub struct WritableDirView {
    pub id: String,
    pub path: String,
    pub created_at: i64,
}

#[derive(Deserialize)]
pub struct NewWritableDir {
    pub path: String,
}

/// Normalizes a user-entered directory into a clean global prefix: strips the
/// leading slash, drops empty and `.` segments, and rejects `..` traversal or
/// the special `~workspace` mount. The result addresses a node in the user's
/// global files.
pub fn normalize_dir(input: &str) -> Result<String, String> {
    let segments: Vec<&str> = input
        .split('/')
        .map(str::trim)
        .filter(|segment| !segment.is_empty() && *segment != ".")
        .collect();
    if segments.contains(&"..") {
        return Err("path must not contain `..`".to_string());
    }
    if segments.is_empty() {
        return Err("path must not be empty".to_string());
    }
    if segments[0] == WORKSPACE_MOUNT {
        return Err("the thread workspace is always writable".to_string());
    }
    Ok(segments.join("/"))
}

pub async fn list(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<WritableDirView>>, WritableDirApiError> {
    let owner = auth::authenticated_user(&state, &headers).await?;
    let rows = writable_dirs::select()
        .where_(writable_dirs::owner.eq(owner))
        .order_by_asc(writable_dirs::path)
        .all(&state.db)
        .await
        .map_err(|_| WritableDirApiError::Internal)?;

    Ok(Json(
        rows.into_iter()
            .map(|row| WritableDirView {
                id: row.id.to_string(),
                path: row.path,
                created_at: row.created_at,
            })
            .collect(),
    ))
}

pub async fn create(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Json(request): Json<NewWritableDir>,
) -> Result<(StatusCode, Json<WritableDirView>), WritableDirApiError> {
    let owner = auth::authenticated_user(&state, &headers).await?;
    let path = normalize_dir(&request.path).map_err(WritableDirApiError::BadRequest)?;

    let duplicate = writable_dirs::select_cols((writable_dirs::id,))
        .where_(
            writable_dirs::owner
                .eq(owner)
                .and(writable_dirs::path.eq(path.as_str())),
        )
        .all(&state.db)
        .await
        .map_err(|_| WritableDirApiError::Internal)?;
    if !duplicate.is_empty() {
        return Err(WritableDirApiError::Conflict);
    }

    let id = Uuid::now_v7();
    let created_at = now();
    writable_dirs::insert()
        .id(id)
        .owner(owner)
        .path(path.as_str())
        .created_at(created_at)
        .execute(&state.db)
        .await
        .map_err(|_| WritableDirApiError::Internal)?;

    Ok((
        StatusCode::CREATED,
        Json(WritableDirView {
            id: id.to_string(),
            path,
            created_at,
        }),
    ))
}

pub async fn delete(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, WritableDirApiError> {
    let owner = auth::authenticated_user(&state, &headers).await?;
    let existing = writable_dirs::select_cols((writable_dirs::id,))
        .where_(writable_dirs::id.eq(id).and(writable_dirs::owner.eq(owner)))
        .all(&state.db)
        .await
        .map_err(|_| WritableDirApiError::Internal)?;
    if existing.is_empty() {
        return Err(WritableDirApiError::NotFound);
    }

    writable_dirs::delete()
        .where_(writable_dirs::id.eq(id).and(writable_dirs::owner.eq(owner)))
        .execute(&state.db)
        .await
        .map_err(|_| WritableDirApiError::Internal)?;
    Ok(StatusCode::NO_CONTENT)
}

/// Loads a user's configured writable directories as normalized global
/// prefixes. Failures degrade to an empty list so a transient database error
/// never silently widens write access.
pub async fn writable_prefixes(db: &ConnectionPool, owner: Uuid) -> Vec<String> {
    writable_dirs::select()
        .where_(writable_dirs::owner.eq(owner))
        .all(db)
        .await
        .map(|rows| rows.into_iter().map(|row| row.path).collect())
        .unwrap_or_default()
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_slashes_and_dot_segments() {
        assert_eq!(normalize_dir("/Documents/").unwrap(), "Documents");
        assert_eq!(normalize_dir("Notes//Personal").unwrap(), "Notes/Personal");
        assert_eq!(normalize_dir("./a/./b").unwrap(), "a/b");
    }

    #[test]
    fn normalize_rejects_traversal_empty_and_workspace() {
        assert!(normalize_dir("a/../b").is_err());
        assert!(normalize_dir("   ").is_err());
        assert!(normalize_dir("/").is_err());
        assert!(normalize_dir(&format!("/{WORKSPACE_MOUNT}/x")).is_err());
    }
}
