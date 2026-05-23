use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    ServerState,
    api::auth::{self, AuthError},
    db::projects,
};

#[derive(Serialize)]
pub struct ProjectResponse {
    pub id: String,
    pub title: String,
}

#[derive(Deserialize)]
pub struct CreateProjectRequest {
    title: String,
}

#[derive(Deserialize)]
pub struct RenameProjectRequest {
    title: String,
}

#[derive(Debug)]
pub enum ProjectApiError {
    Auth(AuthError),
    BadRequest,
    NotFound,
    Internal,
}

impl IntoResponse for ProjectApiError {
    fn into_response(self) -> Response {
        match self {
            ProjectApiError::Auth(error) => error.into_response(),
            ProjectApiError::BadRequest => StatusCode::BAD_REQUEST.into_response(),
            ProjectApiError::NotFound => StatusCode::NOT_FOUND.into_response(),
            ProjectApiError::Internal => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        }
    }
}

impl From<AuthError> for ProjectApiError {
    fn from(error: AuthError) -> Self {
        ProjectApiError::Auth(error)
    }
}

pub async fn list(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<ProjectResponse>>, ProjectApiError> {
    let owner = auth::authenticated_user(&state, &headers).await?;
    let rows = projects::select()
        .where_(projects::owner.eq(owner))
        .order_by_desc(projects::id)
        .all(&state.db)
        .await
        .map_err(|_| ProjectApiError::Internal)?;

    Ok(Json(
        rows.into_iter()
            .map(|r| ProjectResponse {
                id: r.id.to_string(),
                title: r.title,
            })
            .collect(),
    ))
}

pub async fn create(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Json(request): Json<CreateProjectRequest>,
) -> Result<(StatusCode, Json<ProjectResponse>), ProjectApiError> {
    let owner = auth::authenticated_user(&state, &headers).await?;
    let title = request.title.trim().to_string();
    if title.is_empty() {
        return Err(ProjectApiError::BadRequest);
    }

    let id = Uuid::now_v7();
    projects::insert()
        .id(id)
        .owner(owner)
        .title(title.as_str())
        .execute(&state.db)
        .await
        .map_err(|_| ProjectApiError::Internal)?;

    Ok((
        StatusCode::CREATED,
        Json(ProjectResponse {
            id: id.to_string(),
            title,
        }),
    ))
}

pub async fn rename(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Path(project_id): Path<Uuid>,
    Json(request): Json<RenameProjectRequest>,
) -> Result<Json<ProjectResponse>, ProjectApiError> {
    let owner = auth::authenticated_user(&state, &headers).await?;
    let title = request.title.trim().to_string();
    if title.is_empty() {
        return Err(ProjectApiError::BadRequest);
    }

    require_project_owner(&state, owner, project_id).await?;

    state
        .db
        .query_with_params(
            "UPDATE projects SET title = ? WHERE id = ?",
            vec![
                minisql::Value::Text(title.clone()),
                minisql::Value::Uuid(project_id),
            ],
        )
        .await
        .map_err(|_| ProjectApiError::Internal)?;

    Ok(Json(ProjectResponse {
        id: project_id.to_string(),
        title,
    }))
}

pub async fn delete(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Path(project_id): Path<Uuid>,
) -> Result<StatusCode, ProjectApiError> {
    let owner = auth::authenticated_user(&state, &headers).await?;
    require_project_owner(&state, owner, project_id).await?;

    // Unlink threads from this project before deleting it
    state
        .db
        .query_with_params(
            "UPDATE threads SET project_id = NULL WHERE project_id = ?",
            vec![minisql::Value::Uuid(project_id)],
        )
        .await
        .map_err(|_| ProjectApiError::Internal)?;

    state
        .db
        .query_with_params(
            "DELETE FROM projects WHERE id = ?",
            vec![minisql::Value::Uuid(project_id)],
        )
        .await
        .map_err(|_| ProjectApiError::Internal)?;

    Ok(StatusCode::NO_CONTENT)
}

async fn require_project_owner(
    state: &ServerState,
    owner: Uuid,
    project_id: Uuid,
) -> Result<(), ProjectApiError> {
    let rows = projects::select_cols((projects::id,))
        .where_(projects::id.eq(project_id).and(projects::owner.eq(owner)))
        .all(&state.db)
        .await
        .map_err(|_| ProjectApiError::Internal)?;

    if rows.is_empty() {
        Err(ProjectApiError::NotFound)
    } else {
        Ok(())
    }
}
