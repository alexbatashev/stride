use std::{cell::RefCell, path::PathBuf, pin::Pin, rc::Rc, sync::Arc};

use futures::{Stream, StreamExt, stream};
use llm::{API, Anthropic, Ollama, OpenAI};
use minisql::ConnectionPool;
use stride_agent::{
    AgentConfig, AgentError, BaseAgent, EventKind, IdGen, InMemoryInteractionBroker,
    InteractionBroker, MessageRole, ModelRegEntry, ModelRegistry, NoopEventSink, SystemIdGen,
    ThreadEvent, Tool, TurnContext,
    tools::{explorer::make_explorer, file::ReadFileTool, glob::GlobTool, patch::PatchTool},
};
use uuid::Uuid;

use crate::{
    agent::CodeAgent,
    config::{self, Config},
    db::{Role, get_migrations, messages, threads},
};

pub struct LocalAgent {
    db: ConnectionPool,
    workdir: PathBuf,
    agent: stride_agent::BaseAgent,
    thread: Rc<RefCell<ThreadState>>,
    id_gen: Arc<dyn IdGen>,
    broker: Arc<InMemoryInteractionBroker>,
}

#[derive(Default)]
struct ThreadState {
    id: Option<Uuid>,
    next_seq: u64,
}

#[derive(Default)]
struct AssistantMessageState {
    id: Option<Uuid>,
    content: String,
    thinking: Option<String>,
}

const SYSTEM_PROMPT: &str =
    "You are Stride, an AI assistant with deep expertise in software engineering.";

impl LocalAgent {
    pub fn new(config: &Config, db: ConnectionPool, workdir: PathBuf) -> Self {
        let model_registry = create_model_registry(config);

        let id_gen: Arc<dyn IdGen> = Arc::new(SystemIdGen);
        let base_config = Arc::new(AgentConfig {
            model_registry,
            max_iterations: 90,
            usage_observer: Arc::new(stride_agent::NoopUsageObserver),
            id_gen: id_gen.clone(),
            ..Default::default()
        });

        let mut agent = BaseAgent::new(
            "default".to_string(),
            base_config,
            SYSTEM_PROMPT.to_string(),
            vec![],
        );

        register_default_tools(&mut agent);

        Self {
            db,
            workdir,
            agent,
            thread: Rc::new(RefCell::new(ThreadState::default())),
            id_gen,
            broker: Arc::new(InMemoryInteractionBroker::default()),
        }
    }

    async fn ensure_thread(&self) -> Result<Uuid, AgentError> {
        self.db
            .initialize_database(get_migrations())
            .await
            .map_err(db_error)?;

        if let Some(id) = self.thread.borrow().id {
            return Ok(id);
        }

        let id = self.id_gen.new_uuid_v7();
        let base_dir = self.workdir.to_string_lossy().to_string();

        threads::insert()
            .id(id)
            .base_dir(base_dir.as_str())
            .working_dir(base_dir.as_str())
            .execute(&self.db)
            .await
            .map_err(db_error)?;

        self.thread.borrow_mut().id = Some(id);
        Ok(id)
    }

    async fn insert_message(
        &self,
        thread_id: Uuid,
        role: Role,
        content: &str,
        thinking: Option<&str>,
    ) -> Result<Uuid, AgentError> {
        let id = self.id_gen.new_uuid_v7();
        let seq = {
            let mut thread = self.thread.borrow_mut();
            let seq = thread.next_seq;
            thread.next_seq += 1;
            seq
        };

        messages::insert()
            .id(id)
            .parent_thread(thread_id)
            .seq(seq)
            .role(role)
            .content(content)
            .thinking(thinking)
            .execute(&self.db)
            .await
            .map_err(db_error)?;

        Ok(id)
    }

    fn failed_event(&self, run_id: Uuid, error: impl ToString) -> ThreadEvent {
        ThreadEvent {
            id: self.id_gen.new_uuid_v7(),
            run_id,
            agent_path: Vec::new(),
            kind: EventKind::RunFailed {
                error: error.to_string(),
            },
        }
    }
}

fn db_error(err: Box<dyn std::error::Error + Send + Sync>) -> AgentError {
    AgentError::NetworkError(err.to_string())
}

async fn update_message(
    db: &ConnectionPool,
    id: Uuid,
    content: &str,
    thinking: Option<&str>,
) -> Result<(), AgentError> {
    messages::update()
        .content(content)
        .thinking(thinking)
        .where_(messages::id.eq(id))
        .execute(db)
        .await
        .map_err(db_error)?;

    Ok(())
}

fn create_model_registry(config: &Config) -> ModelRegistry {
    let mut model_registry = ModelRegistry::new();

    for (name, m) in &config.models {
        let p = config.providers.get(&m.provider).unwrap();
        let api: API = match p.kind {
            config::Kind::OpenAI => OpenAI::new(&p.url).into(),
            config::Kind::OpenRouter => OpenAI::openrouter(&p.url).into(),
            config::Kind::Anthropic => Anthropic::new(&p.url).into(),
            config::Kind::Ollama => Ollama::new(&p.url).into(),
            config::Kind::OllamaCloud => Ollama::cloud(&p.url).into(),
        };
        let entry = ModelRegEntry {
            api,
            token: p.read_token(&m.provider).unwrap_or("-".to_string()),
            model_name: m.slug.clone(),
            reasoning_effort: m.reasoning_effort(),
            vision: m.vision.unwrap_or(false),
        };
        model_registry.add_model_with_provider(name, entry, m.provider.as_str());
    }

    model_registry
}

fn register_default_tools(agent: &mut BaseAgent) {
    agent.register_tool(GlobTool {});
    agent.allow_tool(GlobTool {}.name());

    agent.register_tool(ReadFileTool {});
    agent.allow_tool(ReadFileTool {}.name());

    agent.register_tool(make_explorer());
    agent.allow_tool("explorer");

    agent.register_tool(PatchTool {});
}

impl CodeAgent for LocalAgent {
    async fn make_turn(&self, message: &str) -> Pin<Box<dyn Stream<Item = ThreadEvent> + 'static>> {
        let run_id = self.id_gen.new_uuid_v7();
        let thread_id = match self.ensure_thread().await {
            Ok(id) => id,
            Err(error) => {
                let event = self.failed_event(run_id, error);
                return Box::pin(stream::once(async move { event }));
            }
        };

        if let Err(err) = self
            .insert_message(thread_id, Role::User, message, None)
            .await
        {
            let event = self.failed_event(run_id, err);
            return Box::pin(stream::once(async move { event }));
        }

        let context = TurnContext::new(run_id, Arc::new(NoopEventSink), self.broker.clone())
            .without_user_message_events();
        let stream = self
            .agent
            .make_turn(message.to_string(), Vec::new(), context)
            .await;
        let db = self.db.clone();
        let thread_state = self.thread.clone();
        let assistant_state = Rc::new(RefCell::new(AssistantMessageState::default()));

        Box::pin(stream.then(move |mut event| {
            let db = db.clone();
            let thread_state = thread_state.clone();
            let assistant_state = assistant_state.clone();

            async move {
                let persistence = match &event.kind {
                    EventKind::MessageStarted {
                        message_id,
                        role: MessageRole::Assistant,
                    } => {
                        let seq = {
                            let mut thread = thread_state.borrow_mut();
                            let seq = thread.next_seq;
                            thread.next_seq += 1;
                            seq
                        };
                        messages::insert()
                            .id(*message_id)
                            .parent_thread(thread_id)
                            .seq(seq)
                            .role(Role::Agent)
                            .content("")
                            .thinking(Option::<&str>::None)
                            .execute(&db)
                            .await
                            .map_err(db_error)
                            .map(|_| {
                                let mut assistant = assistant_state.borrow_mut();
                                assistant.id = Some(*message_id);
                                assistant.content.clear();
                                assistant.thinking = None;
                            })
                    }
                    EventKind::TextDelta { message_id, delta } => {
                        let update = {
                            let mut assistant = assistant_state.borrow_mut();
                            (assistant.id == Some(*message_id)).then(|| {
                                assistant.content.push_str(delta);
                                (assistant.content.clone(), assistant.thinking.clone())
                            })
                        };
                        if let Some((content, thinking)) = update {
                            update_message(&db, *message_id, &content, thinking.as_deref()).await
                        } else {
                            Ok(())
                        }
                    }
                    EventKind::ThinkingDelta { message_id, delta } => {
                        let update = {
                            let mut assistant = assistant_state.borrow_mut();
                            (assistant.id == Some(*message_id)).then(|| {
                                assistant
                                    .thinking
                                    .get_or_insert_with(String::new)
                                    .push_str(delta);
                                (assistant.content.clone(), assistant.thinking.clone())
                            })
                        };
                        if let Some((content, thinking)) = update {
                            update_message(&db, *message_id, &content, thinking.as_deref()).await
                        } else {
                            Ok(())
                        }
                    }
                    EventKind::MessageCommitted { message_id }
                        if assistant_state.borrow().id == Some(*message_id) =>
                    {
                        assistant_state.borrow_mut().id = None;
                        Ok(())
                    }
                    _ => Ok(()),
                };
                if let Err(error) = persistence {
                    event.kind = EventKind::RunFailed {
                        error: error.to_string(),
                    };
                }
                event
            }
        }))
    }

    fn resolve_approval(&self, approval_id: Uuid, approved: bool) -> bool {
        self.broker.resolve_approval(approval_id, approved)
    }

    fn answer_quiz(&self, quiz_id: Uuid, answers: Vec<String>) -> bool {
        self.broker.answer_quiz(quiz_id, answers)
    }
}
