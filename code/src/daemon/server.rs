use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

use capnp::capability::Promise;
use capnp_rpc::pry;
use tokio::sync::Notify;

use crate::{
    agent::{Agent, ConfirmChannel},
    agent_capnp::{agent_daemon, agent_session, event_sink},
    config::Config,
};

pub struct AgentDaemonImpl {
    config: Config,
    session_count: Arc<AtomicUsize>,
    had_connection: Arc<AtomicBool>,
    shutdown: Arc<Notify>,
}

impl AgentDaemonImpl {
    pub fn new(
        config: Config,
        session_count: Arc<AtomicUsize>,
        had_connection: Arc<AtomicBool>,
        shutdown: Arc<Notify>,
    ) -> Self {
        Self {
            config,
            session_count,
            had_connection,
            shutdown,
        }
    }
}

impl agent_daemon::Server for AgentDaemonImpl {
    fn connect(
        &mut self,
        params: agent_daemon::ConnectParams,
        mut results: agent_daemon::ConnectResults,
    ) -> Promise<(), capnp::Error> {
        let sink = pry!(pry!(params.get()).get_sink());
        let session = pry!(
            AgentSessionImpl::new(
                self.config.clone(),
                sink,
                self.session_count.clone(),
                self.had_connection.clone(),
                self.shutdown.clone(),
            )
            .map_err(|e| capnp::Error::failed(e.to_string()))
        );
        // Results::get() returns Builder directly (no Result)
        results.get().set_session(capnp_rpc::new_client(session));
        Promise::ok(())
    }
}

struct AgentSessionImpl {
    agent: Rc<RefCell<Option<Agent>>>,
    confirm_channel: Rc<ConfirmChannel>,
    session_count: Arc<AtomicUsize>,
    had_connection: Arc<AtomicBool>,
    shutdown: Arc<Notify>,
}

impl AgentSessionImpl {
    fn new(
        config: Config,
        sink: event_sink::Client,
        session_count: Arc<AtomicUsize>,
        had_connection: Arc<AtomicBool>,
        shutdown: Arc<Notify>,
    ) -> anyhow::Result<Self> {
        let confirm_channel = Rc::new(ConfirmChannel::new());
        let agent = Agent::from_config(&config, sink, confirm_channel.clone())?;
        session_count.fetch_add(1, Ordering::SeqCst);
        had_connection.store(true, Ordering::SeqCst);
        Ok(Self {
            agent: Rc::new(RefCell::new(Some(agent))),
            confirm_channel,
            session_count,
            had_connection,
            shutdown,
        })
    }
}

impl Drop for AgentSessionImpl {
    fn drop(&mut self) {
        let prev = self.session_count.fetch_sub(1, Ordering::SeqCst);
        if prev == 1 && self.had_connection.load(Ordering::SeqCst) {
            self.shutdown.notify_one();
        }
    }
}

impl agent_session::Server for AgentSessionImpl {
    fn send_message(
        &mut self,
        params: agent_session::SendMessageParams,
        _: agent_session::SendMessageResults,
    ) -> Promise<(), capnp::Error> {
        let text = match extract_text(params) {
            Ok(t) => t,
            Err(e) => return Promise::err(e),
        };
        let agent_cell = self.agent.clone();
        Promise::from_future(async move {
            let mut agent = agent_cell
                .borrow_mut()
                .take()
                .ok_or_else(|| capnp::Error::failed("agent is busy".into()))?;
            let result = agent.send_message(text).await;
            *agent_cell.borrow_mut() = Some(agent);
            result.map_err(|e| capnp::Error::failed(e.to_string()))
        })
    }

    fn send_command(
        &mut self,
        params: agent_session::SendCommandParams,
        _: agent_session::SendCommandResults,
    ) -> Promise<(), capnp::Error> {
        let cmd = match pry!(params.get()).get_command() {
            Ok(r) => r.to_string().unwrap_or_default(),
            Err(e) => return Promise::err(e),
        };
        let agent_cell = self.agent.clone();
        Promise::from_future(async move {
            let mut agent = agent_cell
                .borrow_mut()
                .take()
                .ok_or_else(|| capnp::Error::failed("agent is busy".into()))?;
            let result = agent.execute_command(&cmd).await;
            *agent_cell.borrow_mut() = Some(agent);
            result
                .map(|_| ())
                .map_err(|e| capnp::Error::failed(e.to_string()))
        })
    }

    fn confirm(
        &mut self,
        params: agent_session::ConfirmParams,
        _: agent_session::ConfirmResults,
    ) -> Promise<(), capnp::Error> {
        let answer = pry!(params.get()).get_answer();
        let channel = self.confirm_channel.clone();
        Promise::from_future(async move {
            channel.resolve(answer);
            Ok(())
        })
    }

    fn disconnect(
        &mut self,
        _: agent_session::DisconnectParams,
        _: agent_session::DisconnectResults,
    ) -> Promise<(), capnp::Error> {
        Promise::ok(())
    }
}

fn extract_text(params: agent_session::SendMessageParams) -> Result<String, capnp::Error> {
    // Params::get() returns Result<Reader>
    let reader = params.get()?;
    let text_reader = reader.get_text()?;
    text_reader
        .to_string()
        .map_err(|e| capnp::Error::failed(e.to_string()))
}
