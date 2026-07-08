use std::{cell::RefCell, path::PathBuf, pin::Pin, rc::Rc, sync::Arc};

use futures::{Stream, StreamExt, stream};
use llm::{API, Anthropic, Ollama, OpenAI};
use minisql::ConnectionPool;
use stride_agent::{
    AgentConfig, AgentError, AgentResponseChunk, BaseAgent, ModelRegEntry, ModelRegistry, Tool,
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

        let base_config = Arc::new(AgentConfig {
            model_registry,
            max_iterations: 90,
            observer: Arc::new(stride_agent::NoopAgentObserver),
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

        let id = Uuid::now_v7();
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
        let id = Uuid::now_v7();
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
    async fn make_turn(
        &self,
        message: &str,
    ) -> Pin<Box<dyn Stream<Item = Result<AgentResponseChunk, AgentError>> + 'static>> {
        let thread_id = match self.ensure_thread().await {
            Ok(id) => id,
            Err(err) => return Box::pin(stream::once(async { Err(err) })),
        };

        if let Err(err) = self
            .insert_message(thread_id, Role::User, message, None)
            .await
        {
            return Box::pin(stream::once(async { Err(err) }));
        }

        let stream = self.agent.make_turn(message.to_string(), Vec::new()).await;
        let db = self.db.clone();
        let thread_state = self.thread.clone();
        let assistant_state = Rc::new(RefCell::new(AssistantMessageState::default()));

        Box::pin(stream.then(move |item| {
            let db = db.clone();
            let thread_state = thread_state.clone();
            let assistant_state = assistant_state.clone();

            async move {
                if let Ok(AgentResponseChunk::Chunk(chunk)) = &item {
                    if assistant_state.borrow().id.is_none() {
                        let id = Uuid::now_v7();
                        let seq = {
                            let mut thread = thread_state.borrow_mut();
                            let seq = thread.next_seq;
                            thread.next_seq += 1;
                            seq
                        };

                        messages::insert()
                            .id(id)
                            .parent_thread(thread_id)
                            .seq(seq)
                            .role(Role::Agent)
                            .content("")
                            .thinking(Option::<&str>::None)
                            .execute(&db)
                            .await
                            .map_err(db_error)?;

                        let mut assistant = assistant_state.borrow_mut();
                        assistant.id = Some(id);
                        assistant.content.clear();
                        assistant.thinking = None;
                    }

                    let (id, content, thinking) = {
                        let mut assistant = assistant_state.borrow_mut();

                        for choice in &chunk.choices {
                            if let Some(content) = &choice.text {
                                assistant.content.push_str(content);
                            }

                            if let Some(delta) = &choice.delta {
                                if let Some(content) = &delta.content {
                                    assistant.content.push_str(content);
                                }

                                if let Some(thinking) = &delta.thinking {
                                    assistant
                                        .thinking
                                        .get_or_insert_with(String::new)
                                        .push_str(thinking);
                                }
                            }
                        }

                        (
                            assistant.id,
                            assistant.content.clone(),
                            assistant.thinking.clone(),
                        )
                    };

                    if let Some(id) = id {
                        update_message(&db, id, &content, thinking.as_deref()).await?;
                    }

                    if chunk
                        .choices
                        .iter()
                        .any(|choice| choice.finish_reason.is_some())
                    {
                        assistant_state.borrow_mut().id = None;
                    }
                }

                item
            }
        }))
    }
}
