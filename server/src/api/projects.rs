use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use minisql::Value;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    ServerState,
    api::auth::{self, AuthError},
    db::{projects, threads},
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

    let id = state.id_gen.new_uuid_v7();
    projects::insert()
        .id(id)
        .owner(owner)
        .title(title.as_str())
        .execute(&state.db)
        .await
        .map_err(|_| ProjectApiError::Internal)?;

    // Materialize the project's folder in the user's global files so it is
    // visible everywhere and acts as the writable directory for its threads.
    if let Some(vfs) = &state.vfs
        && let Err(error) = vfs.ensure_project_dir(owner, &title).await
    {
        tracing::warn!(%error, %id, "failed to create project directory");
    }

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

    let old_title = project_title(&state, owner, project_id).await?;

    projects::update()
        .title(title.clone())
        .where_(projects::id.eq(project_id))
        .execute(&state.db)
        .await
        .map_err(|_| ProjectApiError::Internal)?;

    if old_title != title {
        // Keep the project's folder and memory wing aligned with its new name so
        // existing files and memories carry over.
        if let Some(vfs) = &state.vfs
            && let Err(error) = vfs.rename_project_dir(owner, &old_title, &title).await
        {
            tracing::warn!(%error, %project_id, "failed to rename project directory");
        }
        rename_project_wing(&state, owner, &old_title, &title).await;
    }

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
    threads::update()
        .project_id(Option::<Uuid>::None)
        .where_(threads::project_id.eq(project_id))
        .execute(&state.db)
        .await
        .map_err(|_| ProjectApiError::Internal)?;

    projects::delete()
        .where_(projects::id.eq(project_id))
        .execute(&state.db)
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

/// Fetches the project's current title, also asserting ownership.
async fn project_title(
    state: &ServerState,
    owner: Uuid,
    project_id: Uuid,
) -> Result<String, ProjectApiError> {
    let rows = projects::select_cols((projects::title,))
        .where_(projects::id.eq(project_id).and(projects::owner.eq(owner)))
        .all(&state.db)
        .await
        .map_err(|_| ProjectApiError::Internal)?;

    rows.into_iter()
        .next()
        .map(|(title,)| title)
        .ok_or(ProjectApiError::NotFound)
}

/// Renames the memory wing that defaults to a project so its stored memories
/// stay reachable under the project's new name.
async fn rename_project_wing(state: &ServerState, owner: Uuid, old_title: &str, new_title: &str) {
    if let Err(error) = state
        .db
        .query_with_params(
            "UPDATE memory_wings SET name = ? WHERE owner = ? AND name = ?",
            vec![
                Value::Text(new_title.to_string()),
                Value::Uuid(owner),
                Value::Text(old_title.to_string()),
            ],
        )
        .await
    {
        tracing::warn!(%error, "failed to rename project memory wing");
    }
}
