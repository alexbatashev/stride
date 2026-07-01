use std::{env, fs, path::PathBuf};

use clap::Parser;
use minisql::ConnectionPool;
use serde::Deserialize;
use stride_agent::memory::memory_closets;

#[derive(Debug, Parser)]
struct Args {
    #[arg(short = 'c')]
    config_path: PathBuf,
}

#[derive(Debug, Deserialize)]
struct RepairConfig {
    server: Option<RepairServerConfig>,
}

#[derive(Debug, Deserialize)]
struct RepairServerConfig {
    db_url: Option<String>,
    db_path: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let config: RepairConfig = toml::from_str(&fs::read_to_string(&args.config_path)?)?;
    let db = ConnectionPool::new(&db_url(&config))
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;

    db.repair_legacy_vector_column(
        &stride_agent::memory::schema(),
        0,
        memory_closets::TABLE_NAME,
        memory_closets::id.name(),
        memory_closets::embedding.name(),
    )
    .await
    .map_err(|error| anyhow::anyhow!(error.to_string()))?;

    Ok(())
}

fn db_url(config: &RepairConfig) -> String {
    env::var("STRIDE_DATABASE_URL")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| {
            config
                .server
                .as_ref()
                .and_then(|server| server.db_url.as_deref())
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .or_else(|| {
            config
                .server
                .as_ref()
                .and_then(|server| server.db_path.as_deref())
                .filter(|value| !value.is_empty())
                .map(|path| format!("sqlite://{path}"))
        })
        .unwrap_or_else(|| "sqlite://stride.db".to_string())
}
