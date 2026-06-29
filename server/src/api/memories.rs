use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use minisql::Value;
use serde::Serialize;
use serde_json::json;
use uuid::Uuid;

use crate::{
    ServerState,
    api::auth::{self, AuthError},
};

#[derive(Serialize)]
pub struct MemoryWingResponse {
    pub id: String,
    pub name: String,
    pub description: String,
    pub rooms: i64,
    pub memories: i64,
    pub created_at: i64,
}

#[derive(Serialize)]
pub struct MemoryRoomResponse {
    pub id: String,
    pub wing: String,
    pub name: String,
    pub description: String,
    pub memories: i64,
    pub created_at: i64,
}

#[derive(Serialize)]
pub struct MemoryResponse {
    pub id: String,
    pub wing: String,
    pub room: String,
    pub title: String,
    pub summary: String,
    pub content: String,
    pub source: Option<String>,
    pub keywords: String,
    pub created_at: i64,
}

#[derive(Serialize)]
pub struct MemorySettingsResponse {
    pub wings: Vec<MemoryWingResponse>,
    pub rooms: Vec<MemoryRoomResponse>,
    pub memories: Vec<MemoryResponse>,
}

#[derive(Debug)]
pub enum MemoryApiError {
    Auth(AuthError),
    NotFound,
    Internal,
}

impl IntoResponse for MemoryApiError {
    fn into_response(self) -> Response {
        match self {
            MemoryApiError::Auth(error) => error.into_response(),
            MemoryApiError::NotFound => (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "memory not found"})),
            )
                .into_response(),
            MemoryApiError::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "internal server error"})),
            )
                .into_response(),
        }
    }
}

impl From<AuthError> for MemoryApiError {
    fn from(error: AuthError) -> Self {
        MemoryApiError::Auth(error)
    }
}

pub async fn list(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
) -> Result<Json<MemorySettingsResponse>, MemoryApiError> {
    let owner = auth::authenticated_user(&state, &headers).await?;

    let wings = state
        .db
        .query_with_params(
            "SELECT w.id AS id, w.name AS name, w.description AS description, \
             w.created_at AS created_at, \
             COUNT(DISTINCT r.id) AS rooms, COUNT(DISTINCT d.id) AS memories \
             FROM memory_wings w \
             LEFT JOIN memory_rooms r ON r.wing = w.id \
             LEFT JOIN memory_drawers d ON d.room = r.id \
             WHERE w.owner = ? \
             GROUP BY w.id, w.name, w.description, w.created_at \
             ORDER BY w.name ASC",
            vec![Value::Uuid(owner)],
        )
        .await
        .map_err(|_| MemoryApiError::Internal)?;

    let rooms = state
        .db
        .query_with_params(
            "SELECT r.id AS id, w.name AS wing, r.name AS name, \
             r.description AS description, r.created_at AS created_at, \
             COUNT(d.id) AS memories \
             FROM memory_rooms r \
             JOIN memory_wings w ON w.id = r.wing \
             LEFT JOIN memory_drawers d ON d.room = r.id \
             WHERE r.owner = ? \
             GROUP BY r.id, w.name, r.name, r.description, r.created_at \
             ORDER BY w.name ASC, r.name ASC",
            vec![Value::Uuid(owner)],
        )
        .await
        .map_err(|_| MemoryApiError::Internal)?;

    let memories = state
        .db
        .query_with_params(
            "SELECT d.id AS id, w.name AS wing, r.name AS room, \
             d.title AS title, d.content AS content, d.source AS source, d.created_at AS created_at, \
             (SELECT c.summary FROM memory_closets c WHERE c.drawer = d.id LIMIT 1) AS summary, \
             (SELECT c.keywords FROM memory_closets c WHERE c.drawer = d.id LIMIT 1) AS keywords \
             FROM memory_drawers d \
             JOIN memory_rooms r ON r.id = d.room \
             JOIN memory_wings w ON w.id = r.wing \
             WHERE d.owner = ? \
             ORDER BY d.created_at DESC",
            vec![Value::Uuid(owner)],
        )
        .await
        .map_err(|_| MemoryApiError::Internal)?;

    Ok(Json(MemorySettingsResponse {
        wings: wings
            .rows()
            .iter()
            .map(|row| MemoryWingResponse {
                id: row_id(row.get("id")),
                name: row.get_text("name").unwrap_or_default().to_string(),
                description: row.get_text("description").unwrap_or_default().to_string(),
                rooms: row.get_int("rooms").unwrap_or(0),
                memories: row.get_int("memories").unwrap_or(0),
                created_at: row.get_int("created_at").unwrap_or(0),
            })
            .collect(),
        rooms: rooms
            .rows()
            .iter()
            .map(|row| MemoryRoomResponse {
                id: row_id(row.get("id")),
                wing: row.get_text("wing").unwrap_or_default().to_string(),
                name: row.get_text("name").unwrap_or_default().to_string(),
                description: row.get_text("description").unwrap_or_default().to_string(),
                memories: row.get_int("memories").unwrap_or(0),
                created_at: row.get_int("created_at").unwrap_or(0),
            })
            .collect(),
        memories: memories
            .rows()
            .iter()
            .map(|row| MemoryResponse {
                id: row_id(row.get("id")),
                wing: row.get_text("wing").unwrap_or_default().to_string(),
                room: row.get_text("room").unwrap_or_default().to_string(),
                title: row.get_text("title").unwrap_or_default().to_string(),
                summary: row.get_text("summary").unwrap_or_default().to_string(),
                content: row.get_text("content").unwrap_or_default().to_string(),
                source: row.get_text("source").map(str::to_string),
                keywords: row.get_text("keywords").unwrap_or_default().to_string(),
                created_at: row.get_int("created_at").unwrap_or(0),
            })
            .collect(),
    }))
}

pub async fn delete(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, MemoryApiError> {
    let owner = auth::authenticated_user(&state, &headers).await?;
    let existing = state
        .db
        .query_with_params(
            "SELECT id FROM memory_drawers WHERE id = ? AND owner = ?",
            vec![Value::Uuid(id), Value::Uuid(owner)],
        )
        .await
        .map_err(|_| MemoryApiError::Internal)?;
    if existing.is_empty() {
        return Err(MemoryApiError::NotFound);
    }

    state
        .db
        .query_with_params(
            "DELETE FROM memory_closets WHERE drawer = ? AND owner = ?",
            vec![Value::Uuid(id), Value::Uuid(owner)],
        )
        .await
        .map_err(|_| MemoryApiError::Internal)?;
    state
        .db
        .query_with_params(
            "DELETE FROM memory_drawers WHERE id = ? AND owner = ?",
            vec![Value::Uuid(id), Value::Uuid(owner)],
        )
        .await
        .map_err(|_| MemoryApiError::Internal)?;

    Ok(StatusCode::NO_CONTENT)
}

fn row_id(value: Option<&Value>) -> String {
    match value {
        Some(Value::Uuid(id)) => id.to_string(),
        Some(Value::Blob(bytes)) => Uuid::from_slice(bytes)
            .map(|id| id.to_string())
            .unwrap_or_default(),
        Some(Value::Text(text)) => text.clone(),
        _ => String::new(),
    }
}
