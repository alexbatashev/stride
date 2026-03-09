mod auth;
mod config;
mod db;
mod frontend;
mod hybrid;
mod llm;
mod rest_grpc;

use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use minisql::{ConnectionPool, Migration};
use tokio::net::TcpListener;
use crate::hybrid::hybrid;

use config::Config;
use std::env;

#[derive(Clone)]
struct AppState {
    db: Arc<ConnectionPool>,
    jwt_secret: Arc<String>,
}

pub async fn server_main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let config = if let Some(path) = env::args().into_iter().nth(1) {
        let file = std::fs::read_to_string(path)?;
        toml::from_str(&file)?
    } else {
        Config::default()
    };

    let jwt_secret = env::var("FRIDAY_JWT_SECRET").unwrap_or("MY_SECRET_KEY_123".into());

    let db = Arc::new(ConnectionPool::new(&config.server.database_url)?);
    let migrations: Vec<Migration> = db::get_migrations();
    db.initialize_database(migrations).await?;
    let state = Arc::new(AppState {
        db,
        jwt_secret: Arc::new(jwt_secret),
    });

    let frontend_files = frontend::init();

    let rest = frontend::http_router(frontend_files).into_make_service();

    let grpc = axum::Router::new()
        .route_service(
            "/friday.core.rpc.AuthService/{*grpc_method}",
            auth::auth_service(state.clone()),
        )
        .route_service(
            "/friday.core.rpc.HelloService/{*grpc_method}",
            auth::hello_service(state.clone()),
        )
        .route_service(
            "/friday.core.rpc.LanguageModel/{*grpc_method}",
            llm::language_model_service(state.db.clone(), state.jwt_secret.as_ref())?,
        );

    let app = hybrid(rest, grpc);

    let addr = SocketAddr::new(
        IpAddr::V4(config.server.bind_address.parse()?),
        config.server.port,
    );

    axum::serve(TcpListener::bind(addr).await?, app).await?;

    Ok(())
}

// enum CompatTask<T> {
//     Tokio(tokio::task::JoinHandle<T>),
//     Thread(std::thread::JoinHandle<T>),
// }

// fn spawn_blocking_compat<F, T>(f: F) -> CompatTask<T>
// where
//     F: FnOnce() -> T + Send + 'static,
//     T: Send + 'static,
// {
//     if tokio::runtime::Handle::try_current().is_ok() {
//         CompatTask::Tokio(tokio::task::spawn_blocking(f))
//     } else {
//         CompatTask::Thread(std::thread::spawn(f))
//     }
// }

// fn resolve_db_url() -> String {
//     if let Ok(url) = std::env::var("FRIDAY_DB_URL") {
//         return url;
//     }

//     let db_path = if let Ok(path) = std::env::var("FRIDAY_DB_PATH") {
//         std::path::PathBuf::from(path)
//     } else if std::env::var("RUNFILES_DIR").is_ok() {
//         std::env::temp_dir().join("friday").join("auth.db")
//     } else {
//         std::path::PathBuf::from("server/backend/auth.db")
//     };

//     if let Some(parent) = db_path.parent() {
//         let _ = std::fs::create_dir_all(parent);
//     }

//     format!("sqlite:{}", db_path.to_string_lossy())
// }

// pub async fn run_server(addr: SocketAddr) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
//     let jwt_secret =
//         std::env::var("FRIDAY_JWT_SECRET").unwrap_or_else(|_| "dev-insecure-secret".to_string());
//     let db_url = resolve_db_url();
//     run_server_with_shutdown(addr, &db_url, jwt_secret, std::future::pending::<()>()).await
// }

// pub async fn run_server_with_shutdown<F>(
//     addr: SocketAddr,
//     db_url: &str,
//     jwt_secret: String,
//     shutdown: F,
// ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
// where
//     F: Future<Output = ()> + Send + 'static,
// {
//     let db = Arc::new(ConnectionPool::new(db_url)?);
//     let migrations: Vec<Migration> = get_migrations();
//     db.initialize_database(migrations).await?;
//     let llm_service = llm::language_model_service(db.clone(), &jwt_secret)?;

//     let state = Arc::new(AppState {
//         db,
//         jwt_secret: Arc::new(jwt_secret),
//     });

//     let files = frontend::init();

//     let rest_router = frontend::http_router(files);
//     let grpc_router = axum::Router::new()
//         .route_service(
//             "/friday.core.rpc.AuthService/{*grpc_method}",
//             auth::auth_service(state.clone()),
//         )
//         .route_service(
//             "/friday.core.rpc.HelloService/{*grpc_method}",
//             auth::hello_service(state),
//         )
//         .route_service("/friday.core.rpc.LanguageModel/{*grpc_method}", llm_service)
//         .layer(GrpcWebLayer::new());
//     let app = RestGrpcService::new(rest_router, grpc_router);

//     let (shutdown_tx, shutdown_rx) = std::sync::mpsc::channel::<()>();
//     std::thread::spawn(move || {
//         futures::executor::block_on(shutdown);
//         let _ = shutdown_tx.send(());
//     });

//     let serve_task = spawn_blocking_compat(move || -> Result<(), String> {
//         let rt = tokio::runtime::Builder::new_current_thread()
//             .enable_all()
//             .build()
//             .map_err(|e| e.to_string())?;
//         rt.block_on(async move {
//             let listener = tokio::net::TcpListener::bind(addr)
//                 .await
//                 .map_err(|e| e.to_string())?;
//             axum::serve(listener, app.into_make_service())
//                 .with_graceful_shutdown(async move {
//                     let _ = tokio::task::spawn_blocking(move || shutdown_rx.recv()).await;
//                 })
//                 .await
//                 .map_err(|e| e.to_string())
//         })
//     });

//     let serve_result = match serve_task {
//         CompatTask::Tokio(handle) => handle.await.map_err(|e| {
//             Box::new(std::io::Error::other(format!(
//                 "serve task join failed: {e}"
//             ))) as Box<dyn std::error::Error + Send + Sync>
//         })?,
//         CompatTask::Thread(handle) => handle.join().map_err(|_| {
//             Box::new(std::io::Error::other("serve thread panicked"))
//                 as Box<dyn std::error::Error + Send + Sync>
//         })?,
//     };
//     serve_result.map_err(|msg| {
//         Box::new(std::io::Error::other(msg)) as Box<dyn std::error::Error + Send + Sync>
//     })?;

//     Ok(())
// }
