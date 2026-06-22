use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::{
    ServerState,
    api::auth::{self, AuthError},
    db::skills,
    tools::static_skills,
};

#[derive(Serialize)]
pub struct SkillResponse {
    pub id: String,
    pub name: String,
    pub title: String,
    pub description: String,
    pub content: String,
}

#[derive(Deserialize)]
pub struct CreateSkillRequest {
    name: String,
    title: String,
    description: String,
    content: String,
}

#[derive(Deserialize)]
pub struct UpdateSkillRequest {
    title: String,
    description: String,
    content: String,
}

#[derive(Debug)]
pub enum SkillApiError {
    Auth(AuthError),
    BadRequest(String),
    Conflict(String),
    NotFound,
    Internal,
}

impl IntoResponse for SkillApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            SkillApiError::Auth(error) => return error.into_response(),
            SkillApiError::BadRequest(message) => (StatusCode::BAD_REQUEST, message),
            SkillApiError::Conflict(message) => (StatusCode::CONFLICT, message),
            SkillApiError::NotFound => (StatusCode::NOT_FOUND, "skill not found".into()),
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

fn response(row: skills::Row) -> SkillResponse {
    SkillResponse {
        id: row.id.to_string(),
        name: row.name,
        title: row.title,
        description: row.description,
        content: row.content,
    }
}

pub async fn list(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<SkillResponse>>, SkillApiError> {
    let owner = auth::authenticated_user(&state, &headers).await?;
    let rows = skills::select()
        .where_(skills::owner.eq(owner))
        .order_by_asc(skills::name)
        .all(&state.db)
        .await
        .map_err(|_| SkillApiError::Internal)?;
    Ok(Json(rows.into_iter().map(response).collect()))
}

pub async fn create(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Json(request): Json<CreateSkillRequest>,
) -> Result<(StatusCode, Json<SkillResponse>), SkillApiError> {
    let owner = auth::authenticated_user(&state, &headers).await?;
    let name = normalize_name(&request.name)?;
    let title = required(&request.title, "title")?;
    let description = required(&request.description, "description")?;
    let content = required(&request.content, "content")?;

    if static_skills::find_static_skill(&name).is_some() {
        return Err(SkillApiError::Conflict(format!(
            "'{name}' is a built-in skill and cannot be overwritten"
        )));
    }

    let duplicate = skills::select_cols((skills::id,))
        .where_(skills::name.eq(name.as_str()))
        .all(&state.db)
        .await
        .map_err(|_| SkillApiError::Internal)?;
    if !duplicate.is_empty() {
        return Err(SkillApiError::Conflict(
            "a skill with this name already exists".to_string(),
        ));
    }

    let id = Uuid::now_v7();
    skills::insert()
        .id(id)
        .name(name.as_str())
        .title(title.as_str())
        .description(description.as_str())
        .content(content.as_str())
        .owner(Some(owner))
        .execute(&state.db)
        .await
        .map_err(|_| SkillApiError::Internal)?;

    Ok((
        StatusCode::CREATED,
        Json(SkillResponse {
            id: id.to_string(),
            name,
            title,
            description,
            content,
        }),
    ))
}

pub async fn update(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(request): Json<UpdateSkillRequest>,
) -> Result<Json<SkillResponse>, SkillApiError> {
    let owner = auth::authenticated_user(&state, &headers).await?;
    let title = required(&request.title, "title")?;
    let description = required(&request.description, "description")?;
    let content = required(&request.content, "content")?;

    let existing = skills::select()
        .where_(skills::id.eq(id).and(skills::owner.eq(owner)))
        .all(&state.db)
        .await
        .map_err(|_| SkillApiError::Internal)?;
    let Some(current) = existing.into_iter().next() else {
        return Err(SkillApiError::NotFound);
    };

    skills::update()
        .title(title.clone())
        .description(description.clone())
        .content(content.clone())
        .where_(skills::id.eq(id).and(skills::owner.eq(owner)))
        .execute(&state.db)
        .await
        .map_err(|_| SkillApiError::Internal)?;

    Ok(Json(SkillResponse {
        id: id.to_string(),
        name: current.name,
        title,
        description,
        content,
    }))
}

pub async fn delete(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, SkillApiError> {
    let owner = auth::authenticated_user(&state, &headers).await?;
    let existing = skills::select_cols((skills::id,))
        .where_(skills::id.eq(id).and(skills::owner.eq(owner)))
        .all(&state.db)
        .await
        .map_err(|_| SkillApiError::Internal)?;
    if existing.is_empty() {
        return Err(SkillApiError::NotFound);
    }

    skills::delete()
        .where_(skills::id.eq(id).and(skills::owner.eq(owner)))
        .execute(&state.db)
        .await
        .map_err(|_| SkillApiError::Internal)?;
    Ok(StatusCode::NO_CONTENT)
}

fn required(value: &str, field: &str) -> Result<String, SkillApiError> {
    let value = value.trim().to_string();
    if value.is_empty() {
        Err(SkillApiError::BadRequest(format!("{field} is required")))
    } else {
        Ok(value)
    }
}

fn normalize_name(value: &str) -> Result<String, SkillApiError> {
    let name = value.trim();
    if name.len() < 2 || name.len() > 64 {
        return Err(SkillApiError::BadRequest(
            "name must be 2-64 characters".to_string(),
        ));
    }
    let mut chars = name.chars();
    let first = chars
        .next()
        .ok_or_else(|| SkillApiError::BadRequest("name is required".to_string()))?;
    if !first.is_ascii_lowercase() {
        return Err(SkillApiError::BadRequest(
            "name must start with a lowercase letter".to_string(),
        ));
    }
    if !name
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
    {
        return Err(SkillApiError::BadRequest(
            "name may only contain lowercase letters, numbers, and hyphens".to_string(),
        ));
    }
    Ok(name.to_string())
}

#[cfg(test)]
mod tests {
    use super::normalize_name;

    #[test]
    fn accepts_slug_names() {
        assert_eq!(normalize_name("python-debugging").unwrap(), "python-debugging");
        assert_eq!(normalize_name("rust2024").unwrap(), "rust2024");
    }

    #[test]
    fn rejects_invalid_names() {
        assert!(normalize_name("a").is_err());
        assert!(normalize_name("2leading").is_err());
        assert!(normalize_name("Has-Caps").is_err());
        assert!(normalize_name("has spaces").is_err());
    }
}
