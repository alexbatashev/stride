mod term;

use std::{ffi::OsString, path::PathBuf};

use clap::Parser;
use minisql::ConnectionPool;

use crate::{agent::LocalAgent, cli::term::Terminal, config::Config};

pub async fn cli_main() -> anyhow::Result<()> {
    let args = Cli::parse();

    let config_path = args
        .config
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp/config.toml"));

    let config = Config::load(&config_path)?;

    let db_path: String = args
        .db_path
        .map(|p| p.into_string().unwrap())
        .unwrap_or_else(|| "/tmp/friday.db".to_string());

    let db = ConnectionPool::new(&format!("sqlite://{}", db_path)).unwrap();

    let (stream, terminal) = Terminal::new();

    let local = tokio::task::LocalSet::new();
    local
        .run_until(async move {
            tokio::task::spawn_local(async move {
                let _agent = LocalAgent::new(&config, db, PathBuf::new());
                let mut stream = stream;

                loop {
                    tokio::select! {
                        _ = tokio::signal::ctrl_c() => {
                            break;
                        }
                        input = stream.recv() => {

                        }
                    }
                }
            });

            terminal.run().await;
        })
        .await;

    Ok(())
}

#[derive(Parser)]
struct Cli {
    config: Option<OsString>,
    db_path: Option<OsString>,
}
