mod agent;
mod cli;
mod config;
mod tools;

use agent::Agent;
use anyhow::{Context, Result};
use clap::Parser;
use std::path::PathBuf;
use tools::ToolRegistry;
use tools::files::{BashTool, EditFileTool, ListFilesTool, ReadFileTool};

const DEFAULT_CONFIG_FILENAME: &str = "config.toml";

#[derive(Parser, Debug)]
#[command(name = "friday")]
#[command(about = "A CLI coding assistant powered by LLMs", long_about = None)]
struct Args {
    /// Path to config file
    #[arg(short, long, value_name = "FILE")]
    config: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let args = Args::parse();

    let config_path = find_config_file(args.config.as_ref())?;
    let config = config::Config::from_file(
        config_path
            .to_str()
            .context("Config path is not valid UTF-8")?,
    )
    .with_context(|| format!("Failed to load config from {:?}", config_path))?;
    println!("Starting interactive mode...");

    let mut registry = ToolRegistry::new();
    registry.register(ReadFileTool);
    registry.register(ListFilesTool);
    registry.register(EditFileTool);
    registry.register(BashTool);

    let mut agent = Agent::from_config(&config, registry)?;
    agent.run().await?;

    Ok(())
}
/// Find the config file, checking multiple locations in order:
/// 1. CLI argument
/// 2. Current directory (config.toml)
/// 3. XDG_CONFIG_HOME/friday/code.toml (if XDG_CONFIG_HOME is set)
/// 4. ~/.config/friday/code.toml (legacy fallback)
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
         Or specify a config file with: code -c <path>"
    ))
}
