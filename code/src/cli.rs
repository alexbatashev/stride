mod approval;
mod prompt;
mod term;
mod widget;

use std::{ffi::OsString, path::PathBuf, pin::Pin};

use clap::Parser;
use crossterm::style::Color;
use friday_agent::{AgentError, AgentResponseChunk};
use futures::{Stream, StreamExt};
use minisql::ConnectionPool;

use crate::{
    agent::{CodeAgent, LocalAgent},
    config::Config,
};
use term::Terminal;

enum AgentState {
    Idle,
    Running(Pin<Box<dyn Stream<Item = Result<AgentResponseChunk, AgentError>> + 'static>>),
}

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

    let (term_input, term_output, terminal) = Terminal::new();

    let local = tokio::task::LocalSet::new();
    local
        .run_until(async move {
            tokio::task::spawn_local(async move {
                let agent = LocalAgent::new(&config, db, PathBuf::new());
                let mut state = AgentState::Idle;

                'main: loop {
                    match state {
                        AgentState::Idle => {
                            tokio::select! {
                                _ = tokio::signal::ctrl_c() => break 'main,
                                input = term_input.recv() => {
                                    if let Some(input) = input {
                                        term_output.charge_spinner();
                                        state = AgentState::Running(agent.make_turn(&input).await);
                                    }
                                }
                            }
                        }
                        AgentState::Running(ref mut stream) => {
                            let done = tokio::select! {
                                _ = tokio::signal::ctrl_c() => break 'main,
                                chunk = stream.next() => {
                                    match chunk {
                                        Some(Ok(AgentResponseChunk::Chunk(c))) => {
                                            for choice in &c.choices {
                                                if let Some(text) = choice
                                                    .delta
                                                    .as_ref()
                                                    .and_then(|d| d.content.as_deref())
                                                    .or(choice.text.as_deref())
                                                {
                                                    term_output.print(text, None)
                                                }
                                            }
                                            false
                                        }
                                        Some(Ok(AgentResponseChunk::Approval { tool_name, message, approved })) => {
                                            term_output.request_approval(&tool_name, &message, approved).await;
                                            false
                                        }
                                        Some(Err(err)) => {
                                            term_output.print(&format!("\n{err}\n"), Some(Color::Red));
                                            true
                                        }
                                        Some(Ok(AgentResponseChunk::Quiz { answered, .. })) => {
                                            let _ = answered.send(vec![]);
                                            false
                                        }
                                        None => true,
                                    }
                                }
                            };

                            if done {
                                term_output.discharge_spinner();
                                state = AgentState::Idle;
                            }
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
    #[arg(short, long)]
    config: Option<OsString>,
    #[arg(long)]
    db_path: Option<OsString>,
}
