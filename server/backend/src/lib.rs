mod auth;
mod frontend;

use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;

use minisql::{ConnectionPool, Migration, migrations};
use tonic_web::GrpcWebLayer;
use tower_http::cors::CorsLayer;

use crate::auth::AppState;

migrations! {
    auth_schema {
        table users {
            id: uuid::Uuid [PrimaryKey],
            email: String [Unique],
            password_hash: String,
            created_at: i64,
        }

        table server_sessions {
            id: uuid::Uuid [PrimaryKey],
            user_id: uuid::Uuid,
            token_id: uuid::Uuid [Unique],
            revoked_at: Option<i64>,
            created_at: i64,
            expires_at: i64,

            foreign_key(user_id -> users.id);
        }
    }
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

    let http_app = frontend::http_router(static_dir, proto_dir);
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

    let grpc_state = state;
    let grpc_task = spawn_blocking_compat(move || -> Result<(), String> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        rt.block_on(async move {
            tonic::transport::Server::builder()
                .accept_http1(true)
                .layer(CorsLayer::permissive())
                .layer(GrpcWebLayer::new())
                .add_service(auth::auth_service(grpc_state.clone()))
                .add_service(auth::hello_service(grpc_state))
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
