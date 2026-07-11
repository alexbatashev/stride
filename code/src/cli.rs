mod approval;
mod prompt;
mod term;
mod widget;

use std::{ffi::OsString, path::PathBuf, pin::Pin};

use clap::Parser;
use crossterm::style::Color;
use futures::{Stream, StreamExt};
use minisql::ConnectionPool;
use stride_agent::{EventKind, ThreadEvent};

use crate::{
    agent::{CodeAgent, LocalAgent},
    config::Config,
};
use term::Terminal;

enum AgentState {
    Idle,
    Running(Pin<Box<dyn Stream<Item = ThreadEvent> + 'static>>),
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
        .unwrap_or_else(|| "/tmp/stride.db".to_string());

    let db = ConnectionPool::new(&format!("sqlite://{}", db_path)).unwrap();

    let (mut term_input, term_output, terminal) = Terminal::new();

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
                                event = stream.next() => {
                                    match event.map(|event| event.kind) {
                                        Some(EventKind::TextDelta { delta, .. }) => {
                                            term_output.print(&delta, None);
                                            false
                                        }
                                        Some(EventKind::ApprovalRequested { approval_id, tool_call_id, message }) => {
                                            let approved = term_output
                                                .request_approval(&tool_call_id, &message)
                                                .await;
                                            agent.resolve_approval(approval_id, approved);
                                            false
                                        }
                                        Some(EventKind::RunFailed { error }) => {
                                            term_output.print(&format!("\n{error}\n"), Some(Color::Red));
                                            true
                                        }
                                        Some(EventKind::QuizRequested { quiz_id, .. }) => {
                                            agent.answer_quiz(quiz_id, Vec::new());
                                            false
                                        }
                                        Some(EventKind::RunFinished | EventKind::RunCancelled) => true,
                                        Some(_) => false,
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
