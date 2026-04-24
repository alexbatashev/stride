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
use crate::persistence::{ThreadStore, default_socket_path};

pub async fn run(config: Config, socket_path: PathBuf) -> Result<()> {
    if let Some(parent) = socket_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if socket_path.exists() {
        let _ = std::fs::remove_file(&socket_path);
    }
    let database_path = database_path(&config);
    let store = ThreadStore::new(database_path).await?;
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
                                let store = store.clone();
                                let count = session_count.clone();
                                let had = had_connection.clone();
                                let notify = shutdown.clone();
                                spawn_local(async move {
                                    if let Err(e) = handle_connection(stream, config, store, count, had, notify).await {
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
    store: ThreadStore,
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
        store,
        session_count,
        had_connection,
        shutdown,
    ));
    let rpc_system = RpcSystem::new(Box::new(network), Some(bootstrap.client));
    rpc_system.await.map_err(|e| anyhow::anyhow!("{}", e))
}

pub fn socket_path(config: &Config) -> PathBuf {
    let config_path = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    default_socket_path(&config_path, config.daemon.socket_path.as_deref())
}

pub fn database_path(config: &Config) -> PathBuf {
    let config_path = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    crate::persistence::default_database_path(&config_path, config.daemon.database_path.as_deref())
}
