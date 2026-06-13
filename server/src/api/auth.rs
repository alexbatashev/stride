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
    Forbidden,
    Conflict,
    Internal,
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let status = match self {
            AuthError::BadRequest => StatusCode::BAD_REQUEST,
            AuthError::Unauthorized => StatusCode::UNAUTHORIZED,
            AuthError::Forbidden => StatusCode::FORBIDDEN,
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
    if !state.config.allow_registration() {
        return Err(AuthError::Forbidden);
    }

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

    match users.into_iter().next() {
        Some((user_id, password_hash)) if !password_hash.is_empty() => {
            // Local user: in-app auth takes precedence, never falls through to LDAP.
            if !verify_password(&request.password, &password_hash) {
                return Err(AuthError::Unauthorized);
            }
            let resp = create_session(&state, user_id).await?;
            Ok((
                [(header::SET_COOKIE, session_cookie(&resp.token))],
                Json(resp),
            ))
        }
        row => {
            // No local password: try LDAP. `row` carries the existing user_id if the
            // account was previously created by LDAP (empty hash sentinel).
            let existing_id = row.map(|(id, _)| id);
            ldap_login(&state, &request, existing_id).await
        }
    }
}

async fn ldap_login(
    state: &ServerState,
    request: &AuthRequest,
    existing_id: Option<Uuid>,
) -> Result<([(HeaderName, String); 1], Json<AuthResponse>), AuthError> {
    let Some(ldap_cfg) = state.config.server.as_ref().and_then(|s| s.ldap.as_ref()) else {
        return Err(AuthError::Unauthorized);
    };

    // Strict allowlist to prevent DN injection via the username.
    if !request
        .username
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return Err(AuthError::Unauthorized);
    }

    let dn = ldap_cfg
        .user_dn_template
        .replace("{username}", &request.username);

    let (conn, mut ldap) = ldap3::LdapConnAsync::new(&ldap_cfg.url)
        .await
        .map_err(|_| AuthError::Unauthorized)?;
    ldap3::drive!(conn);

    ldap.simple_bind(&dn, &request.password)
        .await
        .map_err(|_| AuthError::Unauthorized)?
        .success()
        .map_err(|_| AuthError::Unauthorized)?;

    // Bind succeeded. Find or create the local shadow record (empty hash sentinel).
    let user_id = if let Some(id) = existing_id {
        id
    } else {
        let id = Uuid::now_v7();
        users::insert()
            .id(id)
            .username(request.username.as_str())
            .password_hash("")
            .execute(&state.db)
            .await
            .map_err(|_| AuthError::Internal)?;
        id
    };

    let resp = create_session(state, user_id).await?;
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

    sessions::delete()
        .where_(sessions::id.eq(session_id))
        .execute(&state.db)
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

pub(crate) async fn authenticated_user(
    state: &ServerState,
    headers: &HeaderMap,
) -> Result<Uuid, AuthError> {
    let token = auth_token(headers)?;
    let claims = decode_token(state, token)?;
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| AuthError::Unauthorized)?;
    let session_id = Uuid::parse_str(&claims.sid).map_err(|_| AuthError::Unauthorized)?;

    let rows = state
        .db
        .query_with_params(
            "SELECT users.id FROM sessions \
             INNER JOIN users ON users.id = sessions.user_id \
             WHERE sessions.id = ? AND sessions.user_id = ? AND sessions.expires_at > ?",
            vec![
                Value::Uuid(session_id),
                Value::Uuid(user_id),
                Value::Integer(now() as i64),
            ],
        )
        .await
        .map_err(|_| AuthError::Internal)?;

    if rows.rows().is_empty() {
        Err(AuthError::Unauthorized)
    } else {
        Ok(user_id)
    }
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
    use std::{collections::HashMap, path::PathBuf};
    use tower::ServiceExt;

    use super::*;
    use crate::{
        config::{Config, Server},
        runner::inproc::InProcessAgentPool,
    };
    use friday_agent::{AgentConfig, DEFAULT_MODEL, ModelRegEntry, ModelRegistry};
    use minisql::ConnectionPool;

    use crate::{app, db, db::sessions};

    #[tokio::test]
    async fn auth_flow_register_login_logout() {
        let db = ConnectionPool::new("sqlite::memory:").unwrap();
        db.initialize_database(db::get_migrations()).await.unwrap();
        let app = test_app(empty_config(), db.clone());

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

        let response = app.clone().oneshot(threads_request(&token)).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

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
            .clone()
            .oneshot(json_request(
                "/api/login",
                r#"{"username":"alice","password":"secret"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let token = response_token(response).await;
        assert!(!token.is_empty());

        crate::db::users::delete()
            .where_(crate::db::users::username.eq("alice"))
            .execute(&db)
            .await
            .unwrap();

        let response = app.oneshot(threads_request(&token)).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn register_returns_forbidden_when_registration_is_disabled() {
        let db = ConnectionPool::new("sqlite::memory:").unwrap();
        db.initialize_database(db::get_migrations()).await.unwrap();

        let app = test_app(
            Config {
                server: Some(Server {
                    db_path: None,
                    listen_addr: None,
                    allow_registration: Some(false),
                    ldap: None,
                    files: None,
                    telegram: None,
                }),
                ..empty_config()
            },
            db,
        );

        let response = app
            .oneshot(json_request(
                "/api/register",
                r#"{"username":"alice","password":"secret"}"#,
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
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

    fn empty_config() -> Config {
        Config {
            providers: HashMap::new(),
            models: HashMap::new(),
            server: None,
            tools: None,
            mcp: HashMap::new(),
        }
    }

    fn test_app(config: Config, db: ConnectionPool) -> axum::Router {
        let model_config = Arc::new(AgentConfig {
            model_registry: mock_model_registry(),
            max_iterations: 2,
        });
        let runner = Arc::new(InProcessAgentPool::new(db.clone(), model_config.clone()));

        app(
            Arc::new(ServerState {
                config,
                db,
                jwt_secret: "test-secret".to_string(),
                runner,
                model_config,
                vfs: None,
                telegram_sessions: Arc::new(crate::api::telegram::TelegramSessions::default()),
            }),
            PathBuf::from(crate::DEFAULT_STATIC_DIR),
        )
    }

    fn logout_request(token: &str) -> Request<Body> {
        Request::post("/api/logout")
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap()
    }

    fn threads_request(token: &str) -> Request<Body> {
        Request::get("/api/threads")
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
