use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde_json::json;
use uuid::Uuid;

use crate::{
    ServerState,
    api::auth::{self, AuthError},
    db::mcp_servers,
    mcp_servers::{
        McpServerInput, McpServerSummary, headers_json, normalize_name, normalize_url, summarize,
    },
};

#[derive(Debug)]
pub enum McpApiError {
    Auth(AuthError),
    BadRequest(String),
    Conflict,
    NotFound,
    Internal,
}

impl IntoResponse for McpApiError {
    fn into_response(self) -> Response {
        match self {
            McpApiError::Auth(error) => error.into_response(),
            McpApiError::BadRequest(message) => {
                (StatusCode::BAD_REQUEST, Json(json!({"error": message}))).into_response()
            }
            McpApiError::Conflict => (
                StatusCode::CONFLICT,
                Json(json!({"error": "an MCP server with this name already exists"})),
            )
                .into_response(),
            McpApiError::NotFound => (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "MCP server not found"})),
            )
                .into_response(),
            McpApiError::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "internal server error"})),
            )
                .into_response(),
        }
    }
}

impl From<AuthError> for McpApiError {
    fn from(error: AuthError) -> Self {
        McpApiError::Auth(error)
    }
}

pub async fn list(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<McpServerSummary>>, McpApiError> {
    let owner = auth::authenticated_user(&state, &headers).await?;
    let rows = mcp_servers::select()
        .where_(mcp_servers::owner.eq(owner))
        .order_by_asc(mcp_servers::name)
        .all(&state.db)
        .await
        .map_err(|_| McpApiError::Internal)?;

    Ok(Json(rows.into_iter().map(summarize).collect()))
}

pub async fn create(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Json(request): Json<McpServerInput>,
) -> Result<(StatusCode, Json<McpServerSummary>), McpApiError> {
    let owner = auth::authenticated_user(&state, &headers).await?;
    let name = normalize_name(&request.name).map_err(McpApiError::BadRequest)?;
    let url = normalize_url(&request.url).map_err(McpApiError::BadRequest)?;
    let headers_json = headers_json(&request).map_err(McpApiError::BadRequest)?;

    let duplicate = mcp_servers::select_cols((mcp_servers::id,))
        .where_(
            mcp_servers::owner
                .eq(owner)
                .and(mcp_servers::name.eq(name.as_str())),
        )
        .all(&state.db)
        .await
        .map_err(|_| McpApiError::Internal)?;
    if !duplicate.is_empty() {
        return Err(McpApiError::Conflict);
    }

    let id = state.id_gen.new_uuid_v7();
    let created_at = state.clock.now_unix_secs();
    let enabled = request.enabled.unwrap_or(true);
    mcp_servers::insert()
        .id(id)
        .owner(owner)
        .name(name.as_str())
        .url(url.as_str())
        .headers_json(headers_json.as_deref())
        .enabled(enabled)
        .created_at(created_at)
        .execute(&state.db)
        .await
        .map_err(|_| McpApiError::Internal)?;

    Ok((
        StatusCode::CREATED,
        Json(McpServerSummary {
            id: id.to_string(),
            name,
            url,
            enabled,
            created_at,
            header_names: Vec::new(),
            has_authorization: headers_json
                .as_deref()
                .is_some_and(|headers| headers.to_ascii_lowercase().contains("authorization")),
        }),
    ))
}

pub async fn delete(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, McpApiError> {
    let owner = auth::authenticated_user(&state, &headers).await?;
    let existing = mcp_servers::select_cols((mcp_servers::id,))
        .where_(mcp_servers::id.eq(id).and(mcp_servers::owner.eq(owner)))
        .all(&state.db)
        .await
        .map_err(|_| McpApiError::Internal)?;
    if existing.is_empty() {
        return Err(McpApiError::NotFound);
    }

    mcp_servers::delete()
        .where_(mcp_servers::id.eq(id).and(mcp_servers::owner.eq(owner)))
        .execute(&state.db)
        .await
        .map_err(|_| McpApiError::Internal)?;
    Ok(StatusCode::NO_CONTENT)
}
