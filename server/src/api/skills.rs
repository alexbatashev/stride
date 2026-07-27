use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    ServerState,
    api::auth::{self, AuthError},
    skills::{Skill, SkillStoreError, UserSkillView},
};

#[derive(Serialize)]
pub struct SkillResponse {
    pub name: String,
    pub description: String,
    pub content: String,
    pub path: String,
    pub valid: bool,
    pub error: Option<String>,
}

impl From<UserSkillView> for SkillResponse {
    fn from(skill: UserSkillView) -> Self {
        Self {
            name: skill.name,
            description: skill.description,
            content: skill.content,
            path: skill.path,
            valid: skill.valid,
            error: skill.error,
        }
    }
}

impl From<Skill> for SkillResponse {
    fn from(skill: Skill) -> Self {
        Self {
            name: skill.manifest.name,
            description: skill.manifest.description,
            content: skill.content,
            path: skill.path,
            valid: true,
            error: None,
        }
    }
}

#[derive(Deserialize)]
pub struct CreateSkillRequest {
    name: String,
    description: String,
    content: String,
}

#[derive(Deserialize)]
pub struct UpdateSkillRequest {
    description: String,
    content: String,
}

#[derive(Debug)]
pub enum SkillApiError {
    Auth(AuthError),
    BadRequest(String),
    Conflict(String),
    NotFound,
    Unavailable,
    Internal,
}

impl IntoResponse for SkillApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            SkillApiError::Auth(error) => return error.into_response(),
            SkillApiError::BadRequest(message) => (StatusCode::BAD_REQUEST, message),
            SkillApiError::Conflict(message) => (StatusCode::CONFLICT, message),
            SkillApiError::NotFound => (StatusCode::NOT_FOUND, "skill not found".into()),
            SkillApiError::Unavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                "user skill storage is unavailable".into(),
            ),
            SkillApiError::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal server error".into(),
            ),
        };
        (status, Json(json!({"error": message}))).into_response()
    }
}

impl From<AuthError> for SkillApiError {
    fn from(error: AuthError) -> Self {
        SkillApiError::Auth(error)
    }
}

impl From<SkillStoreError> for SkillApiError {
    fn from(error: SkillStoreError) -> Self {
        match error {
            SkillStoreError::StorageUnavailable => Self::Unavailable,
            SkillStoreError::Invalid(message) => Self::BadRequest(message),
            SkillStoreError::Conflict(message) => Self::Conflict(message),
            SkillStoreError::NotFound(_) => Self::NotFound,
            SkillStoreError::Internal(_) => Self::Internal,
        }
    }
}

pub async fn list(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<SkillResponse>>, SkillApiError> {
    let owner = auth::authenticated_user(&state, &headers).await?;
    let skills = state.skills.user_views(owner).await?;
    Ok(Json(skills.into_iter().map(Into::into).collect()))
}

pub async fn create(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Json(request): Json<CreateSkillRequest>,
) -> Result<(StatusCode, Json<SkillResponse>), SkillApiError> {
    let owner = auth::authenticated_user(&state, &headers).await?;
    let skill = state
        .skills
        .create(
            owner,
            request.name.trim(),
            request.description.trim(),
            &request.content,
        )
        .await?;
    Ok((StatusCode::CREATED, Json(skill.into())))
}

pub async fn update(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Path(name): Path<String>,
    Json(request): Json<UpdateSkillRequest>,
) -> Result<Json<SkillResponse>, SkillApiError> {
    let owner = auth::authenticated_user(&state, &headers).await?;
    let skill = state
        .skills
        .update(owner, &name, request.description.trim(), &request.content)
        .await?;
    Ok(Json(skill.into()))
}

pub async fn delete(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Result<StatusCode, SkillApiError> {
    let owner = auth::authenticated_user(&state, &headers).await?;
    state.skills.delete(owner, &name).await?;
    Ok(StatusCode::NO_CONTENT)
}
