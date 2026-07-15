use std::sync::Arc;

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    ServerState,
    api::auth::{self, AuthError},
    db::users,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PersonalSettings {
    pub username: String,
    pub full_name: String,
    pub personality: String,
}

#[derive(Deserialize)]
pub struct UpdatePersonalSettings {
    full_name: String,
    personality: String,
}

#[derive(Debug)]
pub enum PersonalApiError {
    Auth(AuthError),
    BadRequest,
    Internal,
}

impl IntoResponse for PersonalApiError {
    fn into_response(self) -> Response {
        match self {
            PersonalApiError::Auth(error) => error.into_response(),
            PersonalApiError::BadRequest => StatusCode::BAD_REQUEST.into_response(),
            PersonalApiError::Internal => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        }
    }
}

impl From<AuthError> for PersonalApiError {
    fn from(error: AuthError) -> Self {
        PersonalApiError::Auth(error)
    }
}

pub async fn get(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
) -> Result<Json<PersonalSettings>, PersonalApiError> {
    let owner = auth::authenticated_user(&state, &headers).await?;
    load(&state, owner)
        .await
        .map(Json)
        .map_err(|_| PersonalApiError::Internal)
}

pub async fn update(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Json(request): Json<UpdatePersonalSettings>,
) -> Result<Json<PersonalSettings>, PersonalApiError> {
    let owner = auth::authenticated_user(&state, &headers).await?;
    let full_name = request.full_name.trim();
    if full_name.is_empty() {
        return Err(PersonalApiError::BadRequest);
    }
    let personality = request.personality.trim();

    users::update()
        .full_name(Some(full_name))
        .personality((!personality.is_empty()).then_some(personality))
        .where_(users::id.eq(owner))
        .execute(&state.db)
        .await
        .map_err(|_| PersonalApiError::Internal)?;

    load(&state, owner)
        .await
        .map(Json)
        .map_err(|_| PersonalApiError::Internal)
}

pub(crate) async fn load(state: &ServerState, owner: Uuid) -> anyhow::Result<PersonalSettings> {
    let (username, full_name, personality) =
        users::select_cols((users::username, users::full_name, users::personality))
            .where_(users::id.eq(owner))
            .one(&state.db)
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;

    Ok(PersonalSettings {
        full_name: full_name.unwrap_or_else(|| username.clone()),
        username,
        personality: personality.unwrap_or_default(),
    })
}
