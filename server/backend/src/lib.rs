mod auth;

use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use argon2::Argon2;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use axum::Router;
use axum::body::Bytes;
use axum::extract::State as AxumState;
use axum::http::{StatusCode, header};
use axum::response::Response as AxumResponse;
use axum::routing::post;
use friday::grpc::generated::friday::core::rpc::{
    AuthReply, HelloReply, HelloRequest, LoginRequest, LogoutReply, LogoutRequest, RegisterRequest,
    auth_service_server::{AuthService, AuthServiceServer},
    hello_service_server::{HelloService, HelloServiceServer},
};
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use minisql::{ConnectionPool, Migration, Value, migrations};
use prost::Message;
use serde::{Deserialize, Serialize};
use tonic::metadata::MetadataMap;
use tonic::{Request, Response, Status};
use tower_http::services::ServeDir;
use uuid::Uuid;

migrations! {
    auth_schema {
        table users {
            id: Uuid [PrimaryKey],
            email: String [Unique],
            password_hash: String,
            created_at: i64,
        }

        table server_sessions {
            id: Uuid [PrimaryKey],
            user_id: Uuid,
            token_id: Uuid [Unique],
            revoked_at: Option<i64>,
            created_at: i64,
            expires_at: i64,

            foreign_key(user_id -> users.id);
        }
    }
}

#[derive(Clone)]
struct AppState {
    db: Arc<ConnectionPool>,
    jwt_secret: Arc<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    sub: String,
    sid: String,
    exp: usize,
    iat: usize,
}

#[derive(Clone)]
struct GreeterService {
    state: Arc<AppState>,
}

#[derive(Clone)]
struct AuthServiceImpl {
    state: Arc<AppState>,
}

enum CompatTask<T> {
    Tokio(tokio::task::JoinHandle<T>),
    Thread(std::thread::JoinHandle<T>),
}

fn spawn_blocking_compat<F, T>(f: F) -> CompatTask<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    if tokio::runtime::Handle::try_current().is_ok() {
        CompatTask::Tokio(tokio::task::spawn_blocking(f))
    } else {
        CompatTask::Thread(std::thread::spawn(f))
    }
}

fn now_epoch_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn bearer_token(metadata: &MetadataMap) -> Result<&str, Status> {
    let raw = metadata
        .get("authorization")
        .ok_or_else(|| Status::unauthenticated("missing authorization metadata"))?;
    let auth = raw
        .to_str()
        .map_err(|_| Status::unauthenticated("invalid authorization metadata"))?;
    let token = auth
        .strip_prefix("Bearer ")
        .ok_or_else(|| Status::unauthenticated("authorization must be Bearer token"))?;
    if token.is_empty() {
        return Err(Status::unauthenticated("empty bearer token"));
    }
    Ok(token)
}

fn hash_password(password: &str) -> Result<String, Status> {
    let salt = SaltString::encode_b64(Uuid::new_v4().as_bytes())
        .map_err(|_| Status::internal("failed to generate password salt"))?;
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|_| Status::internal("failed to hash password"))?
        .to_string();
    Ok(hash)
}

fn verify_password(password: &str, password_hash: &str) -> Result<(), Status> {
    let parsed = PasswordHash::new(password_hash)
        .map_err(|_| Status::unauthenticated("invalid credentials"))?;
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .map_err(|_| Status::unauthenticated("invalid credentials"))
}

fn issue_jwt(state: &AppState, user_id: Uuid) -> Result<(String, Uuid, i64), Status> {
    let now = now_epoch_seconds();
    let expires_at = now + 60 * 60 * 24;
    let session_id = Uuid::new_v4();
    let claims = Claims {
        sub: user_id.to_string(),
        sid: session_id.to_string(),
        iat: now as usize,
        exp: expires_at as usize,
    };

    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(state.jwt_secret.as_bytes()),
    )
    .map_err(|_| Status::internal("failed to sign jwt"))?;

    Ok((token, session_id, expires_at))
}

fn decode_jwt(state: &AppState, token: &str) -> Result<Claims, Status> {
    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(state.jwt_secret.as_bytes()),
        &Validation::default(),
    )
    .map_err(|_| Status::unauthenticated("invalid token"))?;
    Ok(token_data.claims)
}

async fn validate_active_session(state: &AppState, claims: &Claims) -> Result<(), Status> {
    let sid = Uuid::parse_str(&claims.sid).map_err(|_| Status::unauthenticated("invalid token"))?;
    let rows = server_sessions::select()
        .where_(server_sessions::token_id.eq(sid))
        .limit(1)
        .all(&state.db)
        .await
        .map_err(|_| Status::internal("failed to load session"))?;

    let row = rows
        .first()
        .ok_or_else(|| Status::unauthenticated("unknown session"))?;

    if row.user_id.to_string() != claims.sub {
        return Err(Status::unauthenticated("invalid session user"));
    }
    if row.revoked_at.is_some() {
        return Err(Status::unauthenticated("session revoked"));
    }
    if row.expires_at <= now_epoch_seconds() {
        return Err(Status::unauthenticated("session expired"));
    }

    Ok(())
}

#[tonic::async_trait]
impl HelloService for GreeterService {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        let token = bearer_token(request.metadata())?;
        let claims = decode_jwt(&self.state, token)?;
        validate_active_session(&self.state, &claims).await?;

        let name = request.into_inner().name;
        Ok(Response::new(HelloReply {
            message: format!("hello, {} (user {})", name, claims.sub),
        }))
    }
}

#[tonic::async_trait]
impl AuthService for AuthServiceImpl {
    async fn register(
        &self,
        request: Request<RegisterRequest>,
    ) -> Result<Response<AuthReply>, Status> {
        let body = request.into_inner();
        let email = body.email.trim().to_lowercase();
        if email.is_empty() || body.password.len() < 8 {
            return Err(Status::invalid_argument(
                "email must be set and password must be at least 8 characters",
            ));
        }

        let existing = users::select()
            .where_(users::email.eq(email.as_str()))
            .limit(1)
            .all(&self.state.db)
            .await
            .map_err(|_| Status::internal("failed to query users"))?;
        if !existing.is_empty() {
            return Err(Status::already_exists("user already exists"));
        }

        let user_id = Uuid::new_v4();
        let password_hash = hash_password(&body.password)?;
        users::insert()
            .id(user_id)
            .email(email.as_str())
            .password_hash(password_hash.as_str())
            .created_at(now_epoch_seconds())
            .execute(&self.state.db)
            .await
            .map_err(|_| Status::internal("failed to create user"))?;

        let (token, session_id, expires_at) = issue_jwt(&self.state, user_id)?;
        server_sessions::insert()
            .id(Uuid::new_v4())
            .user_id(user_id)
            .token_id(session_id)
            .revoked_at(Option::<i64>::None)
            .created_at(now_epoch_seconds())
            .expires_at(expires_at)
            .execute(&self.state.db)
            .await
            .map_err(|_| Status::internal("failed to create session"))?;

        Ok(Response::new(AuthReply {
            token,
            user_id: user_id.to_string(),
            expires_at,
        }))
    }

    async fn login(&self, request: Request<LoginRequest>) -> Result<Response<AuthReply>, Status> {
        let body = request.into_inner();
        let email = body.email.trim().to_lowercase();
        let users_found = users::select()
            .where_(users::email.eq(email.as_str()))
            .limit(1)
            .all(&self.state.db)
            .await
            .map_err(|_| Status::internal("failed to query users"))?;

        let user = users_found
            .first()
            .ok_or_else(|| Status::unauthenticated("invalid credentials"))?;
        verify_password(&body.password, &user.password_hash)?;

        let (token, session_id, expires_at) = issue_jwt(&self.state, user.id)?;
        server_sessions::insert()
            .id(Uuid::new_v4())
            .user_id(user.id)
            .token_id(session_id)
            .revoked_at(Option::<i64>::None)
            .created_at(now_epoch_seconds())
            .expires_at(expires_at)
            .execute(&self.state.db)
            .await
            .map_err(|_| Status::internal("failed to create session"))?;

        Ok(Response::new(AuthReply {
            token,
            user_id: user.id.to_string(),
            expires_at,
        }))
    }

    async fn logout(
        &self,
        request: Request<LogoutRequest>,
    ) -> Result<Response<LogoutReply>, Status> {
        let body = request.into_inner();
        let token = if body.token.trim().is_empty() {
            return Err(Status::invalid_argument("token is required"));
        } else {
            body.token.trim()
        };

        let claims = decode_jwt(&self.state, token)?;
        let sid =
            Uuid::parse_str(&claims.sid).map_err(|_| Status::unauthenticated("invalid token"))?;
        self.state
            .db
            .query_with_params(
                "UPDATE server_sessions SET revoked_at = ? WHERE token_id = ?",
                vec![Value::Integer(now_epoch_seconds()), Value::Uuid(sid)],
            )
            .await
            .map_err(|_| Status::internal("failed to revoke session"))?;

        Ok(Response::new(LogoutReply { success: true }))
    }
}

fn grpc_web_frame(flag: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(5 + payload.len());
    out.push(flag);
    out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    out.extend_from_slice(payload);
    out
}

fn grpc_web_trailers(status: tonic::Code, message: &str) -> Vec<u8> {
    let trailers = format!(
        "grpc-status: {}\r\ngrpc-message: {}\r\n",
        status as i32, message
    );
    grpc_web_frame(0x80, trailers.as_bytes())
}

fn grpc_web_payload(body: &[u8]) -> Result<&[u8], Status> {
    if body.len() < 5 {
        return Err(Status::invalid_argument("grpc-web body is too short"));
    }
    if (body[0] & 0x80) != 0 {
        return Err(Status::invalid_argument("expected grpc-web data frame"));
    }
    let len = u32::from_be_bytes([body[1], body[2], body[3], body[4]]) as usize;
    if body.len() < 5 + len {
        return Err(Status::invalid_argument("invalid grpc-web frame length"));
    }
    Ok(&body[5..5 + len])
}

fn grpc_web_response(framed: Vec<u8>) -> AxumResponse {
    let mut response = AxumResponse::new(axum::body::Body::from(framed));
    *response.status_mut() = StatusCode::OK;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        "application/grpc-web+proto"
            .parse()
            .expect("valid grpc-web content-type"),
    );
    response.headers_mut().insert(
        header::ACCESS_CONTROL_EXPOSE_HEADERS,
        "grpc-status,grpc-message"
            .parse()
            .expect("valid exposed headers"),
    );
    response
}

async fn grpc_web_register(
    AxumState(state): AxumState<Arc<AppState>>,
    body: Bytes,
) -> AxumResponse {
    let service = AuthServiceImpl { state };
    let payload = match grpc_web_payload(&body) {
        Ok(payload) => payload,
        Err(status) => {
            return grpc_web_response(grpc_web_trailers(status.code(), status.message()));
        }
    };
    let request = match <RegisterRequest as prost::Message>::decode(payload) {
        Ok(request) => request,
        Err(_) => {
            return grpc_web_response(grpc_web_trailers(
                tonic::Code::InvalidArgument,
                "invalid request body",
            ));
        }
    };

    match service.register(Request::new(request)).await {
        Ok(response) => {
            let encoded = response.into_inner().encode_to_vec();
            let mut framed = grpc_web_frame(0, &encoded);
            framed.extend(grpc_web_trailers(tonic::Code::Ok, ""));
            grpc_web_response(framed)
        }
        Err(status) => grpc_web_response(grpc_web_trailers(status.code(), status.message())),
    }
}

async fn grpc_web_login(AxumState(state): AxumState<Arc<AppState>>, body: Bytes) -> AxumResponse {
    let service = AuthServiceImpl { state };
    let payload = match grpc_web_payload(&body) {
        Ok(payload) => payload,
        Err(status) => {
            return grpc_web_response(grpc_web_trailers(status.code(), status.message()));
        }
    };
    let request = match <LoginRequest as prost::Message>::decode(payload) {
        Ok(request) => request,
        Err(_) => {
            return grpc_web_response(grpc_web_trailers(
                tonic::Code::InvalidArgument,
                "invalid request body",
            ));
        }
    };

    match service.login(Request::new(request)).await {
        Ok(response) => {
            let encoded = response.into_inner().encode_to_vec();
            let mut framed = grpc_web_frame(0, &encoded);
            framed.extend(grpc_web_trailers(tonic::Code::Ok, ""));
            grpc_web_response(framed)
        }
        Err(status) => grpc_web_response(grpc_web_trailers(status.code(), status.message())),
    }
}

fn resolve_static_dir() -> String {
    if let Ok(dir) = std::env::var("FRIDAY_STATIC_DIR") {
        return dir;
    }
    // When run via `bazel run`, Bazel sets RUNFILES_DIR pointing to the runfiles tree.
    // Static assets land at <runfiles>/friday/server/frontend/ via the data dep.
    if let Ok(runfiles) = std::env::var("RUNFILES_DIR") {
        let path = format!("{}/friday/server/frontend", runfiles);
        if std::path::Path::new(&path).is_dir() {
            return path;
        }
    }
    "server/frontend".to_string()
}

fn resolve_proto_dir() -> String {
    if let Ok(dir) = std::env::var("FRIDAY_PROTO_DIR") {
        return dir;
    }
    if let Ok(runfiles) = std::env::var("RUNFILES_DIR") {
        let path = format!("{}/friday/libs/core/proto", runfiles);
        if std::path::Path::new(&path).is_dir() {
            return path;
        }
    }
    "libs/core/proto".to_string()
}

fn resolve_db_url() -> String {
    if let Ok(url) = std::env::var("FRIDAY_DB_URL") {
        return url;
    }

    let db_path = if let Ok(path) = std::env::var("FRIDAY_DB_PATH") {
        std::path::PathBuf::from(path)
    } else if std::env::var("RUNFILES_DIR").is_ok() {
        // Under `bazel run`, runfiles are not a safe writable location.
        std::env::temp_dir().join("friday").join("auth.db")
    } else {
        std::path::PathBuf::from("server/backend/auth.db")
    };

    if let Some(parent) = db_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    format!("sqlite:{}", db_path.to_string_lossy())
}

pub async fn run_server(
    grpc_addr: SocketAddr,
    http_addr: SocketAddr,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let jwt_secret =
        std::env::var("FRIDAY_JWT_SECRET").unwrap_or_else(|_| "dev-insecure-secret".to_string());
    let db_url = resolve_db_url();
    let static_dir = resolve_static_dir();
    let proto_dir = resolve_proto_dir();
    run_server_with_shutdown(
        grpc_addr,
        http_addr,
        &db_url,
        jwt_secret,
        static_dir,
        proto_dir,
        std::future::pending::<()>(),
    )
    .await
}

pub async fn run_server_with_shutdown<F>(
    grpc_addr: SocketAddr,
    http_addr: SocketAddr,
    db_url: &str,
    jwt_secret: String,
    static_dir: String,
    proto_dir: String,
    shutdown: F,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    F: Future<Output = ()> + Send + 'static,
{
    let db = Arc::new(ConnectionPool::new(db_url)?);
    let migrations: Vec<Migration> = get_migrations();
    db.initialize_database(migrations).await?;

    let state = Arc::new(AppState {
        db,
        jwt_secret: Arc::new(jwt_secret),
    });

    // Broadcast shutdown to HTTP and gRPC runtimes.
    let (http_shutdown_tx, http_shutdown_rx) = std::sync::mpsc::channel::<()>();
    let (grpc_shutdown_tx, grpc_shutdown_rx) = std::sync::mpsc::channel::<()>();
    std::thread::spawn(move || {
        futures::executor::block_on(shutdown);
        let _ = http_shutdown_tx.send(());
        let _ = grpc_shutdown_tx.send(());
    });

    let http_app = Router::new()
        .route(
            "/grpcweb/friday.core.rpc.AuthService/Register",
            post(grpc_web_register),
        )
        .route(
            "/grpcweb/friday.core.rpc.AuthService/Login",
            post(grpc_web_login),
        )
        .nest_service("/proto", ServeDir::new(proto_dir))
        .with_state(state.clone())
        .fallback_service(ServeDir::new(static_dir));
    let http_task = spawn_blocking_compat(move || -> Result<(), String> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        rt.block_on(async move {
            let listener = tokio::net::TcpListener::bind(http_addr)
                .await
                .map_err(|e| e.to_string())?;
            axum::serve(listener, http_app.into_make_service())
                .with_graceful_shutdown(async move {
                    let _ = tokio::task::spawn_blocking(move || http_shutdown_rx.recv()).await;
                })
                .await
                .map_err(|e| e.to_string())
        })
    });

    let grpc_state = state.clone();
    let grpc_task = spawn_blocking_compat(move || -> Result<(), String> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        rt.block_on(async move {
            tonic::transport::Server::builder()
                .add_service(AuthServiceServer::new(AuthServiceImpl {
                    state: grpc_state.clone(),
                }))
                .add_service(HelloServiceServer::new(GreeterService {
                    state: grpc_state,
                }))
                .serve_with_shutdown(grpc_addr, async move {
                    let _ = tokio::task::spawn_blocking(move || grpc_shutdown_rx.recv()).await;
                })
                .await
                .map_err(|e| e.to_string())
        })
    });

    let http_result = match http_task {
        CompatTask::Tokio(handle) => handle.await.map_err(|e| {
            Box::new(std::io::Error::other(format!("http task join failed: {e}")))
                as Box<dyn std::error::Error + Send + Sync>
        })?,
        CompatTask::Thread(handle) => handle.join().map_err(|_| {
            Box::new(std::io::Error::other("http thread panicked"))
                as Box<dyn std::error::Error + Send + Sync>
        })?,
    };
    http_result.map_err(|msg| {
        Box::new(std::io::Error::other(msg)) as Box<dyn std::error::Error + Send + Sync>
    })?;

    let grpc_result = match grpc_task {
        CompatTask::Tokio(handle) => handle.await.map_err(|e| {
            Box::new(std::io::Error::other(format!("grpc task join failed: {e}")))
                as Box<dyn std::error::Error + Send + Sync>
        })?,
        CompatTask::Thread(handle) => handle.join().map_err(|_| {
            Box::new(std::io::Error::other("grpc thread panicked"))
                as Box<dyn std::error::Error + Send + Sync>
        })?,
    };
    grpc_result.map_err(|msg| {
        Box::new(std::io::Error::other(msg)) as Box<dyn std::error::Error + Send + Sync>
    })?;

    Ok(())
}
