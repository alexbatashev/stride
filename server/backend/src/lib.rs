mod auth;
mod config;
mod db;
mod frontend;
mod hybrid;
mod llm;

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use crate::hybrid::hybrid;
use minisql::{ConnectionPool, Migration};
use tokio::net::TcpListener;

use config::Config;
use std::env;

#[derive(Clone)]
struct AppState {
    pub db: Arc<ConnectionPool>,
    pub jwt_secret: Arc<String>,
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

    let auth = auth::auth_service(state.clone());
    let llm_grpc = llm::language_model_service(&state)?;

    let grpc = tonic::service::Routes::new(auth).add_service(llm_grpc);

    let app = hybrid(rest, grpc);

    let addr = SocketAddr::new(
        IpAddr::V4(config.server.bind_address.parse()?),
        config.server.port,
    );

    axum::serve(TcpListener::bind(addr).await?, app).await?;

    Ok(())
}
