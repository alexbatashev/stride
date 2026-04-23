mod server;

use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

use anyhow::Result;
use capnp_rpc::{RpcSystem, rpc_twoparty_capnp, twoparty};
use tokio::net::UnixListener;
use tokio::sync::Notify;
use tokio::task::spawn_local;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

use crate::agent_capnp::agent_daemon;
use crate::config::Config;

pub async fn run(config: Config, socket_path: PathBuf) -> Result<()> {
    let listener = UnixListener::bind(&socket_path)?;
    let session_count = Arc::new(AtomicUsize::new(0));
    let had_connection = Arc::new(AtomicBool::new(false));
    let shutdown = Arc::new(Notify::new());

    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            loop {
                tokio::select! {
                    result = listener.accept() => {
                        match result {
                            Ok((stream, _)) => {
                                let config = config.clone();
                                let count = session_count.clone();
                                let had = had_connection.clone();
                                let notify = shutdown.clone();
                                spawn_local(async move {
                                    if let Err(e) = handle_connection(stream, config, count, had, notify).await {
                                        eprintln!("connection error: {}", e);
                                    }
                                });
                            }
                            Err(e) => {
                                eprintln!("accept error: {}", e);
                                break;
                            }
                        }
                    }
                    _ = shutdown.notified() => {
                        if session_count.load(Ordering::SeqCst) == 0 {
                            break;
                        }
                    }
                }
            }
        })
        .await;

    let _ = std::fs::remove_file(&socket_path);
    Ok(())
}

async fn handle_connection(
    stream: tokio::net::UnixStream,
    config: Config,
    session_count: Arc<AtomicUsize>,
    had_connection: Arc<AtomicBool>,
    shutdown: Arc<Notify>,
) -> Result<()> {
    let (reader, writer) = tokio::io::split(stream);
    let network = twoparty::VatNetwork::new(
        reader.compat(),
        writer.compat_write(),
        rpc_twoparty_capnp::Side::Server,
        Default::default(),
    );
    let bootstrap: agent_daemon::Client = capnp_rpc::new_client(server::AgentDaemonImpl::new(
        config,
        session_count,
        had_connection,
        shutdown,
    ));
    let rpc_system = RpcSystem::new(Box::new(network), Some(bootstrap.client));
    rpc_system.await.map_err(|e| anyhow::anyhow!("{}", e))
}

pub fn socket_path() -> PathBuf {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let cwd = std::env::current_dir().unwrap_or_default();
    let mut hasher = DefaultHasher::new();
    cwd.hash(&mut hasher);
    let hash = hasher.finish();

    let base = std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());

    base.join(format!("friday-code-{:016x}.sock", hash))
}

