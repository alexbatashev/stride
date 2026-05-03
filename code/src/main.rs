mod agent;
#[allow(unused_parens)]
mod agent_capnp {
    include!(concat!(env!("OUT_DIR"), "/agent_capnp.rs"));
}
mod cli;
mod client;
mod config;
mod daemon;
mod persistence;
mod term;

use crate::client::ClientCommand;
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

const DEFAULT_CONFIG_FILENAME: &str = "config.toml";

#[derive(Parser, Debug)]
#[command(name = "friday")]
#[command(about = "A CLI coding assistant powered by LLMs", long_about = None)]
struct Args {
    /// Path to config file
    #[arg(short, long, value_name = "FILE")]
    config: Option<PathBuf>,

    /// Run as background daemon (used internally)
    #[arg(long, hide = true)]
    daemon: bool,

    /// Daemon socket path (used internally)
    #[arg(long, hide = true)]
    socket: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// List saved conversations for a working directory
    Threads {
        #[arg(long, value_name = "DIR")]
        cwd: Option<PathBuf>,
        #[arg(long, default_value_t = 20)]
        limit: u32,
    },
    /// Print full history for a saved conversation
    History { thread_id: String },
    /// Resume a conversation by thread id
    Resume { thread_id: String },
    /// Resume the latest conversation for a working directory
    Continue {
        #[arg(long, value_name = "DIR")]
        cwd: Option<PathBuf>,
    },
}

impl From<Command> for ClientCommand {
    fn from(value: Command) -> Self {
        match value {
            Command::Threads { cwd, limit } => Self::Threads { cwd, limit },
            Command::History { thread_id } => Self::History { thread_id },
            Command::Resume { thread_id } => Self::Resume { thread_id },
            Command::Continue { cwd } => Self::Continue { cwd },
        }
    }
}

fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let args = Args::parse();

    let config_path = find_config_file(args.config.as_ref())?;
    let config = config::Config::from_file(
        config_path
            .to_str()
            .context("Config path is not valid UTF-8")?,
    )
    .with_context(|| format!("Failed to load config from {:?}", config_path))?;

    let socket_path = args.socket.unwrap_or_else(|| daemon::socket_path(&config));

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    if args.daemon {
        rt.block_on(daemon::run(config, socket_path))
    } else {
        rt.block_on(client::run(
            config,
            socket_path,
            config_path,
            args.command.map(Into::into),
        ))
    }
}

fn find_config_file(cli_config: Option<&PathBuf>) -> Result<PathBuf> {
    if let Some(path) = cli_config {
        if path.exists() {
            return Ok(path.clone());
        }
        return Err(anyhow::anyhow!(
            "Config file specified via CLI not found: {:?}",
            path
        ));
    }

    let current_dir = std::env::current_dir()?;
    let current_config = current_dir.join(DEFAULT_CONFIG_FILENAME);
    if current_config.exists() {
        return Ok(current_config);
    }

    if let Ok(xdg_config) = std::env::var("XDG_CONFIG_HOME") {
        let xdg_path = PathBuf::from(&xdg_config)
            .join("friday")
            .join(DEFAULT_CONFIG_FILENAME);
        if xdg_path.exists() {
            return Ok(xdg_path);
        }
    }

    if let Some(home_dir) = dirs::home_dir() {
        let home_config = home_dir
            .join(".config")
            .join("friday")
            .join(DEFAULT_CONFIG_FILENAME);
        if home_config.exists() {
            return Ok(home_config);
        }
    }

    Err(anyhow::anyhow!(
        "Config file not found. Please create one as:\n\
         - ./config.toml (current directory)\n\
         - $XDG_CONFIG_HOME/friday/config.toml (if XDG_CONFIG_HOME is set)\n\
         - ~/.config/friday/config.toml\n\
         Or specify a config file with: friday -c <path>"
    ))
}
