use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
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
    persistence::ThreadStore,
};

pub struct AgentDaemonImpl {
    config: Config,
    store: ThreadStore,
    session_count: Arc<AtomicUsize>,
    had_connection: Arc<AtomicBool>,
    shutdown: Arc<Notify>,
}

impl AgentDaemonImpl {
    pub fn new(
        config: Config,
        store: ThreadStore,
        session_count: Arc<AtomicUsize>,
        had_connection: Arc<AtomicBool>,
        shutdown: Arc<Notify>,
    ) -> Self {
        Self {
            config,
            store,
            session_count,
            had_connection,
            shutdown,
        }
    }
}

impl agent_daemon::Server for AgentDaemonImpl {
    fn start_session(
        &mut self,
        params: agent_daemon::StartSessionParams,
        mut results: agent_daemon::StartSessionResults,
    ) -> Promise<(), capnp::Error> {
        let params = pry!(params.get());
        let sink = pry!(params.get_sink());
        let cwd = pry!(params.get_cwd())
            .to_string()
            .map_err(|e| capnp::Error::failed(e.to_string()));
        let cwd = pry!(cwd);
        let store = self.store.clone();
        let config = self.config.clone();
        let session_count = self.session_count.clone();
        let had_connection = self.had_connection.clone();
        let shutdown = self.shutdown.clone();
        Promise::from_future(async move {
            let cwd = PathBuf::from(cwd);
            let session = AgentSessionImpl::new_fresh(
                config,
                store,
                sink,
                cwd,
                session_count,
                had_connection,
                shutdown,
            )
            .await
            .map_err(|e| capnp::Error::failed(e.to_string()))?;
            let thread_id = session.thread_id();
            results.get().set_thread_id(&thread_id);
            results.get().set_session(capnp_rpc::new_client(session));
            Ok(())
        })
    }

    fn resume_session(
        &mut self,
        params: agent_daemon::ResumeSessionParams,
        mut results: agent_daemon::ResumeSessionResults,
    ) -> Promise<(), capnp::Error> {
        let params = pry!(params.get());
        let sink = pry!(params.get_sink());
        let thread_id = pry!(params.get_thread_id())
            .to_string()
            .map_err(|e| capnp::Error::failed(e.to_string()));
        let thread_id = pry!(thread_id);
        let store = self.store.clone();
        let config = self.config.clone();
        let session_count = self.session_count.clone();
        let had_connection = self.had_connection.clone();
        let shutdown = self.shutdown.clone();
        Promise::from_future(async move {
            let session = AgentSessionImpl::resume(
                config,
                store,
                sink,
                thread_id,
                session_count,
                had_connection,
                shutdown,
            )
            .await
            .map_err(|e| capnp::Error::failed(e.to_string()))?;
            results.get().set_session(capnp_rpc::new_client(session));
            Ok(())
        })
    }

    fn resume_latest_for_cwd(
        &mut self,
        params: agent_daemon::ResumeLatestForCwdParams,
        mut results: agent_daemon::ResumeLatestForCwdResults,
    ) -> Promise<(), capnp::Error> {
        let params = pry!(params.get());
        let sink = pry!(params.get_sink());
        let cwd = pry!(params.get_cwd())
            .to_string()
            .map_err(|e| capnp::Error::failed(e.to_string()));
        let cwd = pry!(cwd);
        let store = self.store.clone();
        let config = self.config.clone();
        let session_count = self.session_count.clone();
        let had_connection = self.had_connection.clone();
        let shutdown = self.shutdown.clone();
        Promise::from_future(async move {
            let cwd = PathBuf::from(cwd);
            let thread_id = store
                .latest_thread_for_cwd(&cwd)
                .await
                .map_err(|e| capnp::Error::failed(e.to_string()))?
                .ok_or_else(|| capnp::Error::failed("no saved conversations for cwd".into()))?;
            let session = AgentSessionImpl::resume(
                config,
                store,
                sink,
                thread_id.clone(),
                session_count,
                had_connection,
                shutdown,
            )
            .await
            .map_err(|e| capnp::Error::failed(e.to_string()))?;
            results.get().set_thread_id(&thread_id);
            results.get().set_session(capnp_rpc::new_client(session));
            Ok(())
        })
    }

    fn list_threads(
        &mut self,
        params: agent_daemon::ListThreadsParams,
        mut results: agent_daemon::ListThreadsResults,
    ) -> Promise<(), capnp::Error> {
        let params = pry!(params.get());
        let cwd = pry!(params.get_cwd())
            .to_string()
            .map_err(|e| capnp::Error::failed(e.to_string()));
        let cwd = pry!(cwd);
        let limit = params.get_limit();
        let store = self.store.clone();
        Promise::from_future(async move {
            let threads = store
                .list_threads(Path::new(&cwd), limit)
                .await
                .map_err(|e| capnp::Error::failed(e.to_string()))?;
            let mut out = results.get().init_threads(threads.len() as u32);
            for (idx, thread) in threads.iter().enumerate() {
                let mut item = out.reborrow().get(idx as u32);
                item.set_id(&thread.id);
                item.set_cwd(&thread.cwd);
                item.set_updated_at(thread.updated_at);
                item.set_preview(&thread.preview);
            }
            Ok(())
        })
    }

    fn get_thread_history(
        &mut self,
        params: agent_daemon::GetThreadHistoryParams,
        mut results: agent_daemon::GetThreadHistoryResults,
    ) -> Promise<(), capnp::Error> {
        let params = pry!(params.get());
        let thread_id = pry!(params.get_thread_id())
            .to_string()
            .map_err(|e| capnp::Error::failed(e.to_string()));
        let thread_id = pry!(thread_id);
        let store = self.store.clone();
        Promise::from_future(async move {
            let messages = store
                .history(&thread_id)
                .await
                .map_err(|e| capnp::Error::failed(e.to_string()))?;
            let mut out = results.get().init_messages(messages.len() as u32);
            for (idx, message) in messages.iter().enumerate() {
                let mut item = out.reborrow().get(idx as u32);
                item.set_seq(message.seq);
                item.set_role(&message.role);
                item.set_content(&message.content);
                item.set_thinking(&message.thinking);
                item.set_tool_call_id(&message.tool_call_id);
                item.set_created_at(message.created_at);
                item.set_tool_name(&message.tool_name);
            }
            Ok(())
        })
    }
}

struct AgentSessionImpl {
    agent: Rc<RefCell<Option<Agent>>>,
    confirm_channel: Rc<ConfirmChannel>,
    store: ThreadStore,
    config: Config,
    thread_id: Rc<RefCell<String>>,
    cwd: PathBuf,
    session_count: Arc<AtomicUsize>,
    had_connection: Arc<AtomicBool>,
    shutdown: Arc<Notify>,
}

impl AgentSessionImpl {
    async fn new_fresh(
        config: Config,
        store: ThreadStore,
        sink: event_sink::Client,
        cwd: PathBuf,
        session_count: Arc<AtomicUsize>,
        had_connection: Arc<AtomicBool>,
        shutdown: Arc<Notify>,
    ) -> anyhow::Result<Self> {
        let confirm_channel = Rc::new(ConfirmChannel::new());
        let default_model = format!("{}/{}", config.default.provider, config.default.model);
        let thread_id = store
            .create_thread_with_model(&cwd, &[], Some(&default_model))
            .await?;
        let thread_id_cell = Rc::new(RefCell::new(thread_id.clone()));
        let checkpoint = checkpoint_callback(store.clone(), cwd.clone(), thread_id_cell.clone());
        let agent = Agent::from_config(
            &config,
            sink,
            confirm_channel.clone(),
            cwd.clone(),
            None,
            None,
            Some(checkpoint),
        )
        .await?;
        store
            .save_thread_with_model(
                &thread_id,
                &cwd,
                agent.conversation(),
                &HashMap::new(),
                Some(&agent.model_key()),
            )
            .await?;
        session_count.fetch_add(1, Ordering::SeqCst);
        had_connection.store(true, Ordering::SeqCst);
        Ok(Self {
            agent: Rc::new(RefCell::new(Some(agent))),
            confirm_channel,
            store,
            config,
            thread_id: thread_id_cell,
            cwd,
            session_count,
            had_connection,
            shutdown,
        })
    }

    async fn resume(
        config: Config,
        store: ThreadStore,
        sink: event_sink::Client,
        thread_id: String,
        session_count: Arc<AtomicUsize>,
        had_connection: Arc<AtomicBool>,
        shutdown: Arc<Notify>,
    ) -> anyhow::Result<Self> {
        let confirm_channel = Rc::new(ConfirmChannel::new());
        let (cwd, conversation, saved_model) = store
            .load_thread_with_model(&thread_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("thread not found: {}", thread_id))?;
        let thread_id_cell = Rc::new(RefCell::new(thread_id.clone()));
        let checkpoint = checkpoint_callback(store.clone(), cwd.clone(), thread_id_cell.clone());
        let agent = Agent::from_config(
            &config,
            sink,
            confirm_channel.clone(),
            cwd.clone(),
            Some(conversation),
            saved_model,
            Some(checkpoint),
        )
        .await?;
        session_count.fetch_add(1, Ordering::SeqCst);
        had_connection.store(true, Ordering::SeqCst);
        Ok(Self {
            agent: Rc::new(RefCell::new(Some(agent))),
            confirm_channel,
            store,
            config,
            thread_id: thread_id_cell,
            cwd,
            session_count,
            had_connection,
            shutdown,
        })
    }

    fn thread_id(&self) -> String {
        self.thread_id.borrow().clone()
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
        let store = self.store.clone();
        let cwd = self.cwd.clone();
        let thread_id = self.thread_id.clone();
        Promise::from_future(async move {
            let mut agent = agent_cell
                .borrow_mut()
                .take()
                .ok_or_else(|| capnp::Error::failed("agent is busy".into()))?;
            let result = agent.send_message(text).await;
            let snapshot = agent.conversation().to_vec();
            let tool_names = agent.tool_display_names().clone();
            let model = agent.model_key();
            let current_thread_id = thread_id.borrow().clone();
            *agent_cell.borrow_mut() = Some(agent);
            store
                .save_thread_with_model(
                    &current_thread_id,
                    &cwd,
                    &snapshot,
                    &tool_names,
                    Some(&model),
                )
                .await
                .map_err(|e| capnp::Error::failed(e.to_string()))?;
            result.map_err(|e| capnp::Error::failed(e.to_string()))
        })
    }

    fn send_command(
        &mut self,
        params: agent_session::SendCommandParams,
        mut results: agent_session::SendCommandResults,
    ) -> Promise<(), capnp::Error> {
        let cmd = match pry!(params.get()).get_command() {
            Ok(r) => r.to_string().unwrap_or_default(),
            Err(e) => return Promise::err(e),
        };
        let agent_cell = self.agent.clone();
        let store = self.store.clone();
        let cwd = self.cwd.clone();
        let config = self.config.clone();
        let thread_id_cell = self.thread_id.clone();
        Promise::from_future(async move {
            let mut agent = agent_cell
                .borrow_mut()
                .take()
                .ok_or_else(|| capnp::Error::failed("agent is busy".into()))?;
            let (should_exit, response_thread_id) =
                match cmd.split_whitespace().next().unwrap_or("") {
                    "/clear" | "/c" => {
                        agent.reset_conversation();
                        agent
                            .reset_model(&config)
                            .map_err(|e| capnp::Error::failed(e.to_string()))?;
                        let new_thread_id = store
                            .create_thread_with_model(
                                &cwd,
                                agent.conversation(),
                                Some(&agent.model_key()),
                            )
                            .await
                            .map_err(|e| capnp::Error::failed(e.to_string()))?;
                        *thread_id_cell.borrow_mut() = new_thread_id.clone();
                        (false, new_thread_id)
                    }
                    _ => {
                        let current_thread_id = thread_id_cell.borrow().clone();
                        let should_exit = agent
                            .execute_command(&cmd)
                            .await
                            .map_err(|e| capnp::Error::failed(e.to_string()))?;
                        if !should_exit {
                            store
                                .save_thread_with_model(
                                    &current_thread_id,
                                    &cwd,
                                    agent.conversation(),
                                    agent.tool_display_names(),
                                    Some(&agent.model_key()),
                                )
                                .await
                                .map_err(|e| capnp::Error::failed(e.to_string()))?;
                        }
                        (should_exit, current_thread_id)
                    }
                };
            *agent_cell.borrow_mut() = Some(agent);
            let mut result = results.get().init_result();
            result.set_should_exit(should_exit);
            result.set_thread_id(&response_thread_id);
            Ok(())
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
        let snapshot = self.agent.borrow().as_ref().map(|agent| {
            (
                agent.conversation().to_vec(),
                agent.tool_display_names().clone(),
                agent.model_key(),
            )
        });
        let store = self.store.clone();
        let cwd = self.cwd.clone();
        let thread_id = self.thread_id.borrow().clone();
        Promise::from_future(async move {
            if let Some((snapshot, tool_names, model)) = snapshot {
                store
                    .save_thread_with_model(&thread_id, &cwd, &snapshot, &tool_names, Some(&model))
                    .await
                    .map_err(|e| capnp::Error::failed(e.to_string()))?;
            }
            Ok(())
        })
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

fn checkpoint_callback(
    store: ThreadStore,
    cwd: PathBuf,
    thread_id: Rc<RefCell<String>>,
) -> Rc<dyn Fn(Vec<llm::Message>, HashMap<usize, String>, String)> {
    Rc::new(move |messages, tool_names, model| {
        let store = store.clone();
        let cwd = cwd.clone();
        let thread_id = thread_id.borrow().clone();
        tokio::task::spawn_local(async move {
            let _ = store
                .save_thread_with_model(&thread_id, &cwd, &messages, &tool_names, Some(&model))
                .await;
        });
    })
}
