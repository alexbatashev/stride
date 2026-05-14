use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use argon2::{
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
    password_hash::{SaltString, rand_core::OsRng},
};
use axum::{
    Json,
    extract::State,
    http::{HeaderMap, HeaderName, StatusCode, header},
    response::{IntoResponse, Response},
};
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use minisql::Value;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    ServerState,
    db::{sessions, users},
};

const SESSION_TTL_SECONDS: u64 = 60 * 60 * 24 * 30;

#[derive(Deserialize)]
pub struct AuthRequest {
    username: String,
    password: String,
}

#[derive(Serialize)]
pub struct AuthResponse {
    token: String,
}

#[derive(Serialize, Deserialize)]
struct Claims {
    sub: String,
    sid: String,
    exp: usize,
}

#[derive(Debug)]
pub(crate) enum AuthError {
    BadRequest,
    Unauthorized,
    Conflict,
    Internal,
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let status = match self {
            AuthError::BadRequest => StatusCode::BAD_REQUEST,
            AuthError::Unauthorized => StatusCode::UNAUTHORIZED,
            AuthError::Conflict => StatusCode::CONFLICT,
            AuthError::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        };

        status.into_response()
    }
}

pub async fn register(
    State(state): State<Arc<ServerState>>,
    Json(request): Json<AuthRequest>,
) -> Result<([(HeaderName, String); 1], Json<AuthResponse>), AuthError> {
    let request = normalize_request(request)?;

    let existing = users::select_cols((users::id,))
        .where_(users::username.eq(request.username.as_str()))
        .all(&state.db)
        .await
        .map_err(|_| AuthError::Internal)?;

    if !existing.is_empty() {
        return Err(AuthError::Conflict);
    }

    let user_id = Uuid::now_v7();
    let password_hash = hash_password(&request.password)?;

    users::insert()
        .id(user_id)
        .username(request.username.as_str())
        .password_hash(password_hash.as_str())
        .execute(&state.db)
        .await
        .map_err(|_| AuthError::Conflict)?;

    let resp = create_session(&state, user_id).await?;
    Ok((
        [(header::SET_COOKIE, session_cookie(&resp.token))],
        Json(resp),
    ))
}

pub async fn login(
    State(state): State<Arc<ServerState>>,
    Json(request): Json<AuthRequest>,
) -> Result<([(HeaderName, String); 1], Json<AuthResponse>), AuthError> {
    let request = normalize_request(request)?;

    let users = users::select_cols((users::id, users::password_hash))
        .where_(users::username.eq(request.username.as_str()))
        .all(&state.db)
        .await
        .map_err(|_| AuthError::Internal)?;

    let Some((user_id, password_hash)) = users.into_iter().next() else {
        return Err(AuthError::Unauthorized);
    };

    if !verify_password(&request.password, &password_hash) {
        return Err(AuthError::Unauthorized);
    }

    let resp = create_session(&state, user_id).await?;
    Ok((
        [(header::SET_COOKIE, session_cookie(&resp.token))],
        Json(resp),
    ))
}

pub async fn logout(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
) -> Result<StatusCode, AuthError> {
    let token = bearer_token(&headers)?;
    let claims = decode_token(&state, token)?;
    let session_id = Uuid::parse_str(&claims.sid).map_err(|_| AuthError::Unauthorized)?;

    state
        .db
        .query_with_params(
            "DELETE FROM sessions WHERE id = ?",
            vec![Value::Uuid(session_id)],
        )
        .await
        .map_err(|_| AuthError::Internal)?;

    Ok(StatusCode::NO_CONTENT)
}

fn normalize_request(mut request: AuthRequest) -> Result<AuthRequest, AuthError> {
    request.username = request.username.trim().to_string();

    if request.username.is_empty() || request.password.is_empty() {
        return Err(AuthError::BadRequest);
    }

    Ok(request)
}

fn hash_password(password: &str) -> Result<String, AuthError> {
    let salt = SaltString::generate(&mut OsRng);

    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|_| AuthError::Internal)
}

fn verify_password(password: &str, password_hash: &str) -> bool {
    let Ok(hash) = PasswordHash::new(password_hash) else {
        return false;
    };

    Argon2::default()
        .verify_password(password.as_bytes(), &hash)
        .is_ok()
}

async fn create_session(state: &ServerState, user_id: Uuid) -> Result<AuthResponse, AuthError> {
    let session_id = Uuid::now_v7();
    let expires_at = now() + SESSION_TTL_SECONDS;

    sessions::insert()
        .id(session_id)
        .user_id(user_id)
        .expires_at(expires_at as i64)
        .execute(&state.db)
        .await
        .map_err(|_| AuthError::Internal)?;

    let claims = Claims {
        sub: user_id.to_string(),
        sid: session_id.to_string(),
        exp: expires_at as usize,
    };
    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(state.jwt_secret.as_bytes()),
    )
    .map_err(|_| AuthError::Internal)?;

    Ok(AuthResponse { token })
}

pub(crate) fn verify_token(jwt_secret: &str, token: &str) -> Result<(), ()> {
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(jwt_secret.as_bytes()),
        &Validation::default(),
    )
    .map(|_| ())
    .map_err(|_| ())
}

pub(crate) fn authenticated_user(
    state: &ServerState,
    headers: &HeaderMap,
) -> Result<Uuid, AuthError> {
    let token = auth_token(headers)?;
    let claims = decode_token(state, token)?;
    Uuid::parse_str(&claims.sub).map_err(|_| AuthError::Unauthorized)
}

fn session_cookie(token: &str) -> String {
    format!("token={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age={SESSION_TTL_SECONDS}")
}

fn decode_token(state: &ServerState, token: &str) -> Result<Claims, AuthError> {
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(state.jwt_secret.as_bytes()),
        &Validation::default(),
    )
    .map(|data| data.claims)
    .map_err(|_| AuthError::Unauthorized)
}

fn bearer_token(headers: &HeaderMap) -> Result<&str, AuthError> {
    let header = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or(AuthError::Unauthorized)?;

    header
        .strip_prefix("Bearer ")
        .ok_or(AuthError::Unauthorized)
}

fn auth_token(headers: &HeaderMap) -> Result<&str, AuthError> {
    if let Some(token) = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
    {
        return Ok(token);
    }

    headers
        .get(axum::http::header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|cookies| {
            cookies
                .split(';')
                .find_map(|part| part.trim().strip_prefix("token="))
        })
        .ok_or(AuthError::Unauthorized)
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before unix epoch")
        .as_secs()
}

#[cfg(test)]
mod tests {
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode, header},
    };
    use handlebars::Handlebars;
    use std::collections::HashMap;
    use tower::ServiceExt;

    use super::*;
    use crate::{config::Config, runner::inproc::InProcessAgentPool};
    use friday_agent::{AgentConfig, DEFAULT_MODEL, ModelRegEntry, ModelRegistry};
    use minisql::ConnectionPool;

    use crate::{app, db, db::sessions};

    #[tokio::test]
    async fn auth_flow_register_login_logout() {
        let db = ConnectionPool::new("sqlite::memory:").unwrap();
        db.initialize_database(db::get_migrations()).await.unwrap();
        let runner = Arc::new(InProcessAgentPool::new(
            db.clone(),
            Arc::new(AgentConfig {
                model_registry: mock_model_registry(),
                max_iterations: 2,
            }),
        ));

        let templates = Handlebars::new();

        let app = app(Arc::new(ServerState {
            config: Config {
                providers: HashMap::new(),
                models: HashMap::new(),
                server: None,
            },
            db: db.clone(),
            jwt_secret: "test-secret".to_string(),
            runner,
            templates,
        }));

        let response = app
            .clone()
            .oneshot(json_request(
                "/api/register",
                r#"{"username":"alice","password":"secret"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let token = response_token(response).await;

        let response = app
            .clone()
            .oneshot(json_request(
                "/api/register",
                r#"{"username":"alice","password":"secret"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT);

        let response = app.clone().oneshot(logout_request(&token)).await.unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        let active_sessions = sessions::select().all(&db).await.unwrap();
        assert!(active_sessions.is_empty());

        let response = app
            .clone()
            .oneshot(json_request(
                "/api/login",
                r#"{"username":"alice","password":"wrong"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let response = app
            .oneshot(json_request(
                "/api/login",
                r#"{"username":"alice","password":"secret"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(!response_token(response).await.is_empty());
    }

    fn json_request(uri: &str, body: &'static str) -> Request<Body> {
        Request::post(uri)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body))
            .unwrap()
    }

    fn mock_model_registry() -> ModelRegistry {
        let mut registry = ModelRegistry::new();
        registry.add_model(
            DEFAULT_MODEL,
            ModelRegEntry {
                api: llm::Mock::new().into(),
                token: "-".to_string(),
                model_name: "mock-model".to_string(),
                thinking: false,
            },
        );
        registry
    }

    fn logout_request(token: &str) -> Request<Body> {
        Request::post("/api/logout")
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap()
    }

    async fn response_token(response: axum::response::Response) -> String {
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        json["token"].as_str().unwrap().to_string()
    }
}
