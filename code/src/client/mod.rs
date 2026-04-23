mod sink;

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Result;
use capnp_rpc::{RpcSystem, rpc_twoparty_capnp, twoparty};
use tokio::io::AsyncBufReadExt;
use tokio::net::UnixStream;
use tokio::sync::mpsc;
use tokio::task::spawn_local;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

use crate::agent_capnp::{agent_daemon, agent_session};
use crate::cli;
use crate::config::Config;

pub async fn run(config: Config, socket_path: PathBuf, config_path: PathBuf) -> Result<()> {
    ensure_daemon_running(&socket_path, &config_path).await?;

    let local = tokio::task::LocalSet::new();
    local.run_until(run_client(config, socket_path)).await
}

async fn run_client(config: Config, socket_path: PathBuf) -> Result<()> {
    let stream = UnixStream::connect(&socket_path)
        .await
        .map_err(|e| anyhow::anyhow!("Cannot connect to daemon: {}", e))?;

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

    let (confirm_tx, mut confirm_rx) = mpsc::unbounded_channel::<String>();
    let sink_impl = sink::EventSinkImpl::new(config.agent.confirm_destructive, confirm_tx);
    let sink_client = capnp_rpc::new_client(sink_impl);

    // Request::get() returns Builder directly (no Result)
    let mut connect_req = daemon.connect_request();
    connect_req.get().set_sink(sink_client);
    let response = connect_req
        .send()
        .promise
        .await
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    // Response::get() returns Result<Reader>
    let session = response
        .get()
        .map_err(|e| anyhow::anyhow!("{}", e))?
        .get_session()
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    cli::print_welcome();

    let mut stdin = tokio::io::BufReader::new(tokio::io::stdin());

    loop {
        cli::print_prompt();

        let mut line = String::new();
        match stdin.read_line(&mut line).await {
            Ok(0) => break,
            Ok(_) => {}
            Err(e) => return Err(anyhow::anyhow!("stdin error: {}", e)),
        }

        let input = line.trim().to_string();
        if input.is_empty() {
            continue;
        }

        if input.starts_with('/') {
            match input.as_str() {
                "/quit" | "/q" => break,
                "/help" | "/h" => {
                    cli::print_help();
                    continue;
                }
                cmd => {
                    let mut req = session.send_command_request();
                    req.get().set_command(cmd);
                    await_rpc(req.send().promise, &session, &mut confirm_rx, &mut stdin).await?;
                }
            }
        } else {
            let mut req = session.send_message_request();
            req.get().set_text(&input);
            await_rpc(req.send().promise, &session, &mut confirm_rx, &mut stdin).await?;
        }
    }

    session.disconnect_request().send().promise.await.ok();
    Ok(())
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
                return result.map(|_| ()).map_err(|e| anyhow::anyhow!("{}", e));
            }
            Some(prompt) = confirm_rx.recv() => {
                let answer = if prompt.is_empty() {
                    true  // auto-approve when confirm_destructive is false
                } else {
                    println!();
                    cli::print_confirm_prompt(&prompt);
                    let mut ans = String::new();
                    stdin.read_line(&mut ans).await?;
                    matches!(ans.trim().to_lowercase().as_str(), "y" | "yes")
                };
                let mut req = session.confirm_request();
                // Request::get() returns Builder directly
                req.get().set_answer(answer);
                req.send().promise.await.map_err(|e| anyhow::anyhow!("{}", e))?;
            }
        }
    }
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
        .spawn()?;

    for _ in 0..20 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if UnixStream::connect(socket_path).await.is_ok() {
            return Ok(());
        }
    }

    Err(anyhow::anyhow!("Daemon failed to start within 2 seconds"))
}
