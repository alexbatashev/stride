use std::sync::Arc;

use axum::{
    Json,
    body::{Body, Bytes, to_bytes},
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use futures::StreamExt;
use llm::{CompletionRequest, ModelDesc};
use serde::Serialize;

use crate::{
    ServerState,
    api::auth::{self, AuthError},
};

const BODY_LIMIT: usize = 1_048_576;

#[derive(Serialize)]
pub struct ModelListResponse {
    object: &'static str,
    data: Vec<ModelDesc>,
}

#[derive(Debug)]
pub enum OpenAiApiError {
    Auth(AuthError),
    BadRequest(String),
    NotFound,
    Upstream(llm::Error),
}

impl IntoResponse for OpenAiApiError {
    fn into_response(self) -> Response {
        match self {
            OpenAiApiError::Auth(error) => error.into_response(),
            OpenAiApiError::BadRequest(message) => {
                json_error(StatusCode::BAD_REQUEST, "invalid_request_error", &message)
            }
            OpenAiApiError::NotFound => {
                json_error(StatusCode::NOT_FOUND, "not_found_error", "model not found")
            }
            OpenAiApiError::Upstream(error) => match error {
                llm::Error::ServerError(status) => json_error(
                    status_code(status),
                    "upstream_error",
                    &format!("upstream returned status {status}"),
                ),
                llm::Error::ServerErrorWithMessage(status, message) => {
                    json_error(status_code(status), "upstream_error", &message)
                }
                other => json_error(
                    StatusCode::BAD_GATEWAY,
                    "upstream_error",
                    &other.to_string(),
                ),
            },
        }
    }
}

impl From<AuthError> for OpenAiApiError {
    fn from(error: AuthError) -> Self {
        OpenAiApiError::Auth(error)
    }
}

impl From<llm::Error> for OpenAiApiError {
    fn from(error: llm::Error) -> Self {
        OpenAiApiError::Upstream(error)
    }
}

pub async fn list_models(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
) -> Result<Json<ModelListResponse>, OpenAiApiError> {
    auth::authenticated_user(&state, &headers).await?;

    let data = state
        .model_config
        .model_registry
        .entries()
        .map(|(id, entry)| model_desc(id, entry.vision))
        .collect();

    Ok(Json(ModelListResponse {
        object: "list",
        data,
    }))
}

pub async fn get_model(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Path(model): Path<String>,
) -> Result<Json<ModelDesc>, OpenAiApiError> {
    auth::authenticated_user(&state, &headers).await?;

    let Some(entry) = state.model_config.model_registry.get(&model) else {
        return Err(OpenAiApiError::NotFound);
    };

    Ok(Json(model_desc(&model, entry.vision)))
}

pub async fn chat_completion(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    body: Body,
) -> Result<Response, OpenAiApiError> {
    auth::authenticated_user(&state, &headers).await?;

    let body = to_bytes(body, BODY_LIMIT)
        .await
        .map_err(|error| OpenAiApiError::BadRequest(error.to_string()))?;
    let request: CompletionRequest = serde_json::from_slice(&body)
        .map_err(|error| OpenAiApiError::BadRequest(error.to_string()))?;

    let Some(entry) = state
        .model_config
        .model_registry
        .get(&request.model)
        .cloned()
    else {
        return Err(OpenAiApiError::NotFound);
    };

    let mut upstream_request = request;
    upstream_request.model = entry.model_name.clone();
    if upstream_request.reasoning_effort.is_none() {
        upstream_request.reasoning_effort = entry.reasoning_effort;
    }

    if upstream_request.stream.unwrap_or(false) {
        return Ok(stream_response(entry, upstream_request));
    }

    let completion = entry
        .api
        .get_completion(&entry.token, upstream_request)
        .await?;

    Ok(Json(completion).into_response())
}

fn model_desc(id: &str, vision: bool) -> ModelDesc {
    let mut supported_parameters = vec![
        "stream".to_string(),
        "tools".to_string(),
        "tool_choice".to_string(),
        "reasoning_effort".to_string(),
    ];
    if vision {
        supported_parameters.push("image_url".to_string());
    }

    ModelDesc {
        id: id.to_string(),
        canonical_slug: None,
        created: None,
        owned_by: Some("stride".to_string()),
        context_length: None,
        supported_parameters,
        description: None,
        name: Some(id.to_string()),
    }
}

fn stream_response(entry: stride_agent::ModelRegEntry, request: CompletionRequest) -> Response {
    let stream = entry
        .api
        .stream_completion(&entry.token, request)
        .map(|chunk| {
            let payload = match chunk {
                Ok(chunk) => match serde_json::to_string(&chunk) {
                    Ok(json) => format!("data: {json}\n\n"),
                    Err(error) => format!(
                        "event: error\ndata: {}\n\n",
                        serde_json::json!({ "error": error.to_string() })
                    ),
                },
                Err(error) => format!(
                    "event: error\ndata: {}\n\n",
                    serde_json::json!({ "error": error.to_string() })
                ),
            };
            Ok::<Bytes, std::io::Error>(Bytes::from(payload))
        });
    let done = futures::stream::once(async {
        Ok::<Bytes, std::io::Error>(Bytes::from_static(b"data: [DONE]\n\n"))
    });

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from_stream(stream.chain(done)))
        .unwrap()
}

fn status_code(status: u16) -> StatusCode {
    StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY)
}

fn json_error(status: StatusCode, kind: &str, message: &str) -> Response {
    (
        status,
        Json(serde_json::json!({
            "error": {
                "message": message,
                "type": kind,
                "param": null,
                "code": null,
            }
        })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_desc_advertises_vision_when_enabled() {
        let desc = model_desc("default", true);

        assert_eq!(desc.id, "default");
        assert!(desc.supported_parameters.contains(&"stream".to_string()));
        assert!(desc.supported_parameters.contains(&"image_url".to_string()));
    }

    #[test]
    fn model_desc_omits_vision_when_disabled() {
        let desc = model_desc("default", false);

        assert!(!desc.supported_parameters.contains(&"image_url".to_string()));
    }
}
