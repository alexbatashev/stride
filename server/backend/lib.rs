use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use argon2::Argon2;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use friday::grpc::generated::friday::core::rpc::{
    AuthReply, HelloReply, HelloRequest, LoginRequest, LogoutReply, LogoutRequest, RegisterRequest,
    auth_service_server::{AuthService, AuthServiceServer},
    hello_service_server::{HelloService, HelloServiceServer},
};
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use minisql::{ConnectionPool, Migration, Value, migrations};
use serde::{Deserialize, Serialize};
use tonic::metadata::MetadataMap;
use tonic::{Request, Response, Status};
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

pub async fn run_server(addr: SocketAddr) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let jwt_secret =
        std::env::var("FRIDAY_JWT_SECRET").unwrap_or_else(|_| "dev-insecure-secret".to_string());
    run_server_with_shutdown(
        addr,
        "sqlite://server/backend/auth.db",
        jwt_secret,
        std::future::pending::<()>(),
    )
    .await
}

pub async fn run_server_with_shutdown<F>(
    addr: SocketAddr,
    db_url: &str,
    jwt_secret: String,
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

    tonic::transport::Server::builder()
        .add_service(AuthServiceServer::new(AuthServiceImpl {
            state: state.clone(),
        }))
        .add_service(HelloServiceServer::new(GreeterService { state }))
        .serve_with_shutdown(addr, shutdown)
        .await?;
    Ok(())
}
