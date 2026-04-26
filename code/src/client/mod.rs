mod popup;
mod sink;

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use capnp_rpc::{RpcSystem, rpc_twoparty_capnp, twoparty};
use llm::API;
use tokio::io::AsyncBufReadExt;
use tokio::net::UnixStream;
use tokio::sync::mpsc;
use tokio::task::spawn_local;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

use crate::agent_capnp::{agent_daemon, agent_session};
use crate::cli;
use crate::config::{Config, ProviderConfig, ProviderType};

#[derive(Debug, Clone)]
pub enum ClientCommand {
    Threads { cwd: Option<PathBuf>, limit: u32 },
    History { thread_id: String },
    Resume { thread_id: String },
    Continue { cwd: Option<PathBuf> },
}

pub async fn run(
    config: Config,
    socket_path: PathBuf,
    config_path: PathBuf,
    command: Option<ClientCommand>,
) -> Result<()> {
    ensure_daemon_running(&socket_path, &config_path).await?;

    let local = tokio::task::LocalSet::new();
    local
        .run_until(run_client(config, socket_path, command))
        .await
}

async fn run_client(
    config: Config,
    socket_path: PathBuf,
    command: Option<ClientCommand>,
) -> Result<()> {
    let daemon = connect_daemon(&socket_path).await?;

    match command {
        None => start_interactive_new(daemon, config).await,
        Some(ClientCommand::Resume { thread_id }) => {
            start_interactive_resume(daemon, config, &thread_id).await
        }
        Some(ClientCommand::Continue { cwd }) => {
            let cwd = normalize_cwd(cwd.unwrap_or(std::env::current_dir()?))?;
            start_interactive_continue(daemon, config, &cwd).await
        }
        Some(ClientCommand::Threads { cwd, limit }) => {
            let cwd = normalize_cwd(cwd.unwrap_or(std::env::current_dir()?))?;
            list_threads(daemon, &cwd, limit).await
        }
        Some(ClientCommand::History { thread_id }) => show_history(daemon, &thread_id).await,
    }
}

async fn connect_daemon(socket_path: &Path) -> Result<agent_daemon::Client> {
    let stream = UnixStream::connect(socket_path)
        .await
        .map_err(|e| anyhow!("Cannot connect to daemon: {}", e))?;

    let (reader, writer) = tokio::io::split(stream);
    let network = twoparty::VatNetwork::new(
        reader.compat(),
        writer.compat_write(),
        rpc_twoparty_capnp::Side::Client,
        Default::default(),
    );
    let mut rpc_system = RpcSystem::new(Box::new(network), None);
    let daemon: agent_daemon::Client = rpc_system.bootstrap(rpc_twoparty_capnp::Side::Server);
    spawn_local(rpc_system);
    Ok(daemon)
}

async fn start_interactive_new(daemon: agent_daemon::Client, config: Config) -> Result<()> {
    let cwd = normalize_cwd(std::env::current_dir()?)?;
    let (session, thread_id, mut confirm_rx) = start_session(&daemon, &config, &cwd).await?;
    run_repl(session, &thread_id, &mut confirm_rx, &config).await
}

async fn start_interactive_resume(
    daemon: agent_daemon::Client,
    config: Config,
    thread_id: &str,
) -> Result<()> {
    let (session, mut confirm_rx) = resume_session(&daemon, &config, thread_id).await?;
    print_full_history(&daemon, thread_id).await?;
    run_repl(session, thread_id, &mut confirm_rx, &config).await
}

async fn start_interactive_continue(
    daemon: agent_daemon::Client,
    config: Config,
    cwd: &Path,
) -> Result<()> {
    let (session, thread_id, mut confirm_rx) = resume_latest(&daemon, &config, cwd).await?;
    print_full_history(&daemon, &thread_id).await?;
    run_repl(session, &thread_id, &mut confirm_rx, &config).await
}

async fn list_threads(daemon: agent_daemon::Client, cwd: &Path, limit: u32) -> Result<()> {
    let mut req = daemon.list_threads_request();
    req.get().set_cwd(&cwd.to_string_lossy());
    req.get().set_limit(limit);
    let response = req.send().promise.await.map_err(|e| anyhow!("{}", e))?;
    let reader = response.get().map_err(|e| anyhow!("{}", e))?;
    let threads = reader.get_threads().map_err(|e| anyhow!("{}", e))?;

    let mut rows = Vec::new();
    for idx in 0..threads.len() {
        let thread = threads.get(idx);
        rows.push((
            thread
                .get_id()
                .map_err(|e| anyhow!("{}", e))?
                .to_string()
                .map_err(|e| anyhow!("{}", e))?,
            thread.get_updated_at(),
            thread
                .get_preview()
                .map_err(|e| anyhow!("{}", e))?
                .to_string()
                .map_err(|e| anyhow!("{}", e))?,
        ));
    }

    cli::print_threads(&cwd.to_string_lossy(), &rows);
    Ok(())
}

async fn show_history(daemon: agent_daemon::Client, thread_id: &str) -> Result<()> {
    print_full_history(&daemon, thread_id).await
}

async fn print_full_history(daemon: &agent_daemon::Client, thread_id: &str) -> Result<()> {
    let mut req = daemon.get_thread_history_request();
    req.get().set_thread_id(thread_id);
    let response = req.send().promise.await.map_err(|e| anyhow!("{}", e))?;
    let reader = response.get().map_err(|e| anyhow!("{}", e))?;
    let messages = reader.get_messages().map_err(|e| anyhow!("{}", e))?;

    let mut rows = Vec::new();
    for idx in 0..messages.len() {
        let message = messages.get(idx);
        rows.push((
            message
                .get_role()
                .map_err(|e| anyhow!("{}", e))?
                .to_string()
                .map_err(|e| anyhow!("{}", e))?,
            message
                .get_content()
                .map_err(|e| anyhow!("{}", e))?
                .to_string()
                .map_err(|e| anyhow!("{}", e))?,
            message
                .get_thinking()
                .map_err(|e| anyhow!("{}", e))?
                .to_string()
                .map_err(|e| anyhow!("{}", e))?,
            message
                .get_tool_call_id()
                .map_err(|e| anyhow!("{}", e))?
                .to_string()
                .map_err(|e| anyhow!("{}", e))?,
            message
                .get_tool_name()
                .map_err(|e| anyhow!("{}", e))?
                .to_string()
                .map_err(|e| anyhow!("{}", e))?,
        ));
    }

    cli::print_transcript(thread_id, &rows);
    Ok(())
}

async fn run_repl(
    session: agent_session::Client,
    initial_thread_id: &str,
    confirm_rx: &mut mpsc::UnboundedReceiver<String>,
    config: &Config,
) -> Result<()> {
    cli::print_welcome(initial_thread_id);
    let mut current_thread_id = initial_thread_id.to_string();
    let mut stdin = tokio::io::BufReader::new(tokio::io::stdin());
    let mut model_choices: Option<Vec<popup::PopupItem>> = None;

    loop {
        let prompt = cli::prompt_text(&current_thread_id);
        let line_result = popup::read_line(&prompt, model_choices.as_deref(), || async {
            let choices = load_model_choices(config).await?;
            Ok::<_, anyhow::Error>(choices)
        })
        .await;
        let line = match line_result {
            Ok(line) => line,
            Err(e) => {
                cli::print_error(&e.to_string());
                continue;
            }
        };
        let popup::ReadLineResult { text, loaded_items } = line;
        if model_choices.is_none() && loaded_items.is_some() {
            model_choices = loaded_items;
        }
        let Some(line) = text else {
            break;
        };

        let input = line.trim().to_string();
        if input.is_empty() {
            continue;
        }

        if input.starts_with('/') {
            match input.as_str() {
                "/help" | "/h" => {
                    cli::print_help();
                    continue;
                }
                cmd => {
                    let mut req = session.send_command_request();
                    req.get().set_command(cmd);
                    let result =
                        await_command_rpc(req.send().promise, &session, confirm_rx, &mut stdin)
                            .await?;
                    if result.thread_id != current_thread_id {
                        current_thread_id = result.thread_id.clone();
                        cli::print_thread_switched(&current_thread_id);
                    }
                    if result.should_exit {
                        break;
                    }
                }
            }
        } else {
            let mut req = session.send_message_request();
            req.get().set_text(&input);
            await_rpc(req.send().promise, &session, confirm_rx, &mut stdin).await?;
        }
    }

    session.disconnect_request().send().promise.await.ok();
    Ok(())
}

async fn load_model_choices(config: &Config) -> Result<Vec<popup::PopupItem>> {
    let mut out = Vec::new();
    let mut errors = Vec::new();
    for provider in &config.providers {
        let api = create_api(provider);
        let Some(token) = provider.api_key.clone() else {
            errors.push(format!("{}: missing API key", provider.name));
            continue;
        };
        let models = match api.list_models(&token).await {
            Ok(models) => models,
            Err(e) => {
                errors.push(format!("{}: {}", provider.name, e));
                if provider.name == config.default.provider {
                    let value = format!("{}/{}", provider.name, config.default.model);
                    out.push(popup::PopupItem {
                        display: format!("{}  configured default", value),
                        value,
                    });
                }
                continue;
            }
        };
        for model in models {
            let value = format!("{}/{}", provider.name, model.id);
            let display = match model.name {
                Some(name) if name != model.id => format!("{}  {}", value, name),
                _ => value.clone(),
            };
            out.push(popup::PopupItem { value, display });
        }
    }
    out.sort_by(|a, b| a.value.cmp(&b.value));
    out.dedup_by(|a, b| a.value == b.value);
    if out.is_empty() && !errors.is_empty() {
        return Err(anyhow!("failed to list models: {}", errors.join("; ")));
    }
    if out.is_empty() {
        return Err(anyhow!("no models available"));
    }
    Ok(out)
}

fn create_api(provider: &ProviderConfig) -> API {
    match provider.provider_type {
        ProviderType::OpenAi => llm::OpenAI::new(&provider.base_url),
        ProviderType::Anthropic => llm::Anthropic::new(&provider.base_url),
        ProviderType::Ollama => llm::Ollama::new(&provider.base_url),
    }
}

async fn start_session(
    daemon: &agent_daemon::Client,
    config: &Config,
    cwd: &Path,
) -> Result<(
    agent_session::Client,
    String,
    mpsc::UnboundedReceiver<String>,
)> {
    let (sink_client, confirm_rx) = make_sink(config);
    let mut req = daemon.start_session_request();
    req.get().set_sink(sink_client);
    req.get().set_cwd(&cwd.to_string_lossy());
    let response = req.send().promise.await.map_err(|e| anyhow!("{}", e))?;
    let reader = response.get().map_err(|e| anyhow!("{}", e))?;
    let session = reader.get_session().map_err(|e| anyhow!("{}", e))?;
    let thread_id = reader
        .get_thread_id()
        .map_err(|e| anyhow!("{}", e))?
        .to_string()
        .map_err(|e| anyhow!("{}", e))?;
    Ok((session, thread_id, confirm_rx))
}

async fn resume_session(
    daemon: &agent_daemon::Client,
    config: &Config,
    thread_id: &str,
) -> Result<(agent_session::Client, mpsc::UnboundedReceiver<String>)> {
    let (sink_client, confirm_rx) = make_sink(config);
    let mut req = daemon.resume_session_request();
    req.get().set_sink(sink_client);
    req.get().set_thread_id(thread_id);
    let response = req.send().promise.await.map_err(|e| anyhow!("{}", e))?;
    let session = response
        .get()
        .map_err(|e| anyhow!("{}", e))?
        .get_session()
        .map_err(|e| anyhow!("{}", e))?;
    Ok((session, confirm_rx))
}

async fn resume_latest(
    daemon: &agent_daemon::Client,
    config: &Config,
    cwd: &Path,
) -> Result<(
    agent_session::Client,
    String,
    mpsc::UnboundedReceiver<String>,
)> {
    let (sink_client, confirm_rx) = make_sink(config);
    let mut req = daemon.resume_latest_for_cwd_request();
    req.get().set_sink(sink_client);
    req.get().set_cwd(&cwd.to_string_lossy());
    let response = req.send().promise.await.map_err(|e| anyhow!("{}", e))?;
    let reader = response.get().map_err(|e| anyhow!("{}", e))?;
    let session = reader.get_session().map_err(|e| anyhow!("{}", e))?;
    let thread_id = reader
        .get_thread_id()
        .map_err(|e| anyhow!("{}", e))?
        .to_string()
        .map_err(|e| anyhow!("{}", e))?;
    Ok((session, thread_id, confirm_rx))
}

fn make_sink(
    config: &Config,
) -> (
    crate::agent_capnp::event_sink::Client,
    mpsc::UnboundedReceiver<String>,
) {
    let (confirm_tx, confirm_rx) = mpsc::unbounded_channel::<String>();
    let sink_impl = sink::EventSinkImpl::new(config.agent.confirm_destructive, confirm_tx);
    let sink_client = capnp_rpc::new_client(sink_impl);
    (sink_client, confirm_rx)
}

fn normalize_cwd(path: PathBuf) -> Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path
    } else {
        std::env::current_dir()?.join(path)
    };

    Ok(absolute.canonicalize().unwrap_or(absolute))
}

/// Drive an in-flight RPC call, handling confirmation requests that arrive mid-flight.
async fn await_rpc<R>(
    promise: capnp::capability::Promise<capnp::capability::Response<R>, capnp::Error>,
    session: &agent_session::Client,
    confirm_rx: &mut mpsc::UnboundedReceiver<String>,
    stdin: &mut tokio::io::BufReader<tokio::io::Stdin>,
) -> Result<()>
where
    R: capnp::traits::Pipelined + capnp::traits::OwnedStruct + 'static,
{
    futures::pin_mut!(promise);
    loop {
        tokio::select! {
            result = &mut promise => {
                return result.map(|_| ()).map_err(|e| anyhow!("{}", e));
            }
            Some(prompt) = confirm_rx.recv() => {
                send_confirmation(session, stdin, &prompt).await?;
            }
        }
    }
}

async fn await_command_rpc(
    promise: capnp::capability::Promise<
        capnp::capability::Response<agent_session::send_command_results::Owned>,
        capnp::Error,
    >,
    session: &agent_session::Client,
    confirm_rx: &mut mpsc::UnboundedReceiver<String>,
    stdin: &mut tokio::io::BufReader<tokio::io::Stdin>,
) -> Result<CommandOutcome> {
    futures::pin_mut!(promise);
    loop {
        tokio::select! {
            result = &mut promise => {
                let response = result.map_err(|e| anyhow!("{}", e))?;
                let reader = response.get().map_err(|e| anyhow!("{}", e))?;
                let result = reader.get_result().map_err(|e| anyhow!("{}", e))?;
                return Ok(CommandOutcome {
                    should_exit: result.get_should_exit(),
                    thread_id: result.get_thread_id()
                        .map_err(|e| anyhow!("{}", e))?
                        .to_string()
                        .map_err(|e| anyhow!("{}", e))?,
                });
            }
            Some(prompt) = confirm_rx.recv() => {
                send_confirmation(session, stdin, &prompt).await?;
            }
        }
    }
}

async fn send_confirmation(
    session: &agent_session::Client,
    stdin: &mut tokio::io::BufReader<tokio::io::Stdin>,
    prompt: &str,
) -> Result<()> {
    let answer = if prompt.is_empty() {
        true
    } else {
        println!();
        cli::print_confirm_prompt(prompt);
        let mut ans = String::new();
        stdin.read_line(&mut ans).await?;
        matches!(ans.trim().to_lowercase().as_str(), "y" | "yes")
    };
    let mut req = session.confirm_request();
    req.get().set_answer(answer);
    req.send().promise.await.map_err(|e| anyhow!("{}", e))?;
    Ok(())
}

async fn ensure_daemon_running(socket_path: &Path, config_path: &Path) -> Result<()> {
    if UnixStream::connect(socket_path).await.is_ok() {
        return Ok(());
    }

    let exe = std::env::current_exe()?;
    tokio::process::Command::new(&exe)
        .args([
            "--daemon",
            "--socket",
            socket_path.to_str().unwrap_or(""),
            "--config",
            config_path.to_str().unwrap_or(""),
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .with_context(|| "failed to spawn daemon")?;

    for _ in 0..20 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if UnixStream::connect(socket_path).await.is_ok() {
            return Ok(());
        }
    }

    Err(anyhow!("Daemon failed to start within 2 seconds"))
}

struct CommandOutcome {
    should_exit: bool,
    thread_id: String,
}
