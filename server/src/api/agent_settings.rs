use std::sync::Arc;

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use uuid::Uuid;

use crate::{
    ServerState,
    api::auth::{self, AuthError},
    model_registry::{self, AgentSettings},
};

#[derive(Debug)]
pub enum AgentSettingsApiError {
    Auth(AuthError),
    BadRequest,
    Internal,
}

impl IntoResponse for AgentSettingsApiError {
    fn into_response(self) -> Response {
        match self {
            AgentSettingsApiError::Auth(error) => error.into_response(),
            AgentSettingsApiError::BadRequest => StatusCode::BAD_REQUEST.into_response(),
            AgentSettingsApiError::Internal => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        }
    }
}

impl From<AuthError> for AgentSettingsApiError {
    fn from(error: AuthError) -> Self {
        AgentSettingsApiError::Auth(error)
    }
}

pub async fn get(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
) -> Result<Json<AgentSettings>, AgentSettingsApiError> {
    let owner = auth::authenticated_user(&state, &headers).await?;
    model_registry::load_agent_settings(&state.config, &state.db, owner)
        .await
        .map(Json)
        .map_err(|_| AgentSettingsApiError::Internal)
}

pub async fn update(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Json(request): Json<AgentSettings>,
) -> Result<Json<AgentSettings>, AgentSettingsApiError> {
    let owner = auth::authenticated_user(&state, &headers).await?;
    let available = model_registry::list_available_models(&state.config, &state.db, owner)
        .await
        .map_err(|_| AgentSettingsApiError::Internal)?;
    let available_keys: std::collections::HashSet<_> =
        available.iter().map(|model| model.key.as_str()).collect();

    for key in &request.subagent_allowed_models {
        if !available_keys.contains(key.as_str()) {
            return Err(AgentSettingsApiError::BadRequest);
        }
    }

    let settings = AgentSettings {
        subagent_allowed_models: request.subagent_allowed_models,
        subagent_guidelines: request.subagent_guidelines.trim().to_string(),
    };

    save(&state, owner, &settings)
        .await
        .map_err(|_| AgentSettingsApiError::Internal)?;

    Ok(Json(settings))
}

async fn save(
    state: &ServerState,
    owner: Uuid,
    settings: &AgentSettings,
) -> anyhow::Result<()> {
    model_registry::save_agent_settings(&state.db, owner, settings).await
}
