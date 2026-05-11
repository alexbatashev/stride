mod approval;
mod prompt;
mod term;
mod widget;

use std::{collections::VecDeque, ffi::OsString, path::PathBuf, pin::Pin};

use clap::Parser;
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

    let (mut term_input, term_output, terminal) = Terminal::new();

    let local = tokio::task::LocalSet::new();
    local
        .run_until(async move {
            tokio::task::spawn_local(async move {
                let agent = LocalAgent::new(&config, db, PathBuf::new());
                let mut state = AgentState::Idle;
                let mut queue: VecDeque<String> = VecDeque::new();

                'main: loop {
                    if matches!(state, AgentState::Idle) {
                        if let Some(input) = queue.pop_front() {
                            state = AgentState::Running(agent.make_turn(&input).await);
                            continue;
                        }
                    }

                    let done = if let AgentState::Running(ref mut stream) = state {
                        tokio::select! {
                            _ = tokio::signal::ctrl_c() => break 'main,
                            input = term_input.recv() => {
                                if let Some(s) = input { queue.push_back(s); }
                                false
                            }
                            chunk = stream.next() => {
                                match chunk {
                                    Some(Ok(AgentResponseChunk::Chunk(c))) => {
                                        for choice in &c.choices {
                                            if let Some(text) = choice.delta.as_ref().and_then(|d| d.content.as_deref()) {
                                                term_output.print(text, None)
                                            }
                                        }
                                        false
                                    }
                                    Some(Ok(AgentResponseChunk::Approval { message, approved })) => {
                                        term_output.request_approval(&message, approved).await;
                                        false
                                    }
                                    _ => true,
                                }
                            }
                        }
                    } else {
                        tokio::select! {
                            _ = tokio::signal::ctrl_c() => break 'main,
                            input = term_input.recv() => {
                                if let Some(s) = input { queue.push_back(s); }
                            }
                        };
                        false
                    };

                    if done {
                        state = AgentState::Idle;
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
