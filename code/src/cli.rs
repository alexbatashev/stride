mod term;

use std::{ffi::OsString, path::PathBuf};

use clap::Parser;
use minisql::ConnectionPool;
use tokio::select;

use crate::{agent::LocalAgent, cli::term::Terminal, config::Config};

pub async fn cli_main() -> anyhow::Result<()> {
    let args = Cli::parse();

    let config_path = args
        .config
        .map(|p| PathBuf::from(p))
        .or_else(|| Some(PathBuf::from("/tmp/config.toml")))
        .unwrap();

    let config = Config::load(&config_path)?;

    let db_path: String = args
        .db_path
        .map(|p| p.into_string().unwrap())
        .or_else(|| Some("/tmp/friday.db".to_string()))
        .unwrap();

    let db = ConnectionPool::new(&format!("sqlite://{}", db_path)).unwrap();

    let (stream, terminal) = Terminal::new();

    tokio::spawn(async move {
        let agent = LocalAgent::new(&config, db, PathBuf::new());

        loop {
            select! {
                _ = tokio::signal::ctrl_c() => {
                    break;
                }
            }
        }
    });

    terminal.run().await;

    Ok(())
}

#[derive(Parser)]
struct Cli {
    config: Option<OsString>,
    db_path: Option<OsString>,
}
