use std::{path::PathBuf, sync::Arc};

use friday_agent::{
    AgentConfig, BaseAgent, ModelRegEntry, ModelRegistry, Tool,
    tools::{
        explorer::{EXPLORER_NAME, make_explorer},
        file::ReadFileTool,
        glob::GlobTool,
        patch::PatchTool,
    },
};
use llm::{Anthropic, Ollama, OpenAI};
use minisql::ConnectionPool;

use crate::{
    agent::CodeAgent,
    config::{self, Config},
};

pub struct LocalAgent {
    db: ConnectionPool,
    workdir: PathBuf,
    agent: friday_agent::BaseAgent,
}

const SYSTEM_PROMPT: &str =
    "You are Friday, an AI assistant with deep expertise in software engineering.";

impl LocalAgent {
    pub fn new(config: &Config, db: ConnectionPool, workdir: PathBuf) -> Self {
        let model_registry = create_model_registry(config);

        let base_config = Arc::new(AgentConfig {
            model_registry,
            max_iterations: 90,
        });

        let mut agent = BaseAgent::new(
            "default".to_string(),
            base_config,
            SYSTEM_PROMPT.to_string(),
            vec![],
        );

        register_default_tools(&mut agent);

        Self { db, workdir, agent }
    }
}

fn create_model_registry(config: &Config) -> ModelRegistry {
    let mut model_registry = ModelRegistry::new();

    for (name, m) in &config.models {
        let p = config.providers.get(&m.provider).unwrap();
        let api = match p.kind {
            config::Kind::OpenAI => OpenAI::new(&p.url),
            config::Kind::Anthropic => Anthropic::new(&p.url),
            config::Kind::Ollama => Ollama::new(&p.url),
        };
        let entry = ModelRegEntry {
            api,
            token: p.read_token(&m.provider).unwrap_or("-".to_string()),
            model_name: m.slug.clone(),
            thinking: m.thinking.unwrap_or(true),
        };
        model_registry.add_model(name, entry);
    }

    model_registry
}

fn register_default_tools(agent: &mut BaseAgent) {
    agent.register_tool(GlobTool {});
    agent.allow_tool(GlobTool {}.name());

    agent.register_tool(ReadFileTool {});
    agent.allow_tool(ReadFileTool {}.name());

    agent.register_tool(make_explorer());
    agent.allow_tool(EXPLORER_NAME);

    agent.register_tool(PatchTool {});
}

impl CodeAgent for LocalAgent {
    fn get_messages(&self) -> Vec<super::Message> {
        todo!()
    }

    async fn make_turn(
        &self,
        message: &str,
    ) -> std::pin::Pin<
        Box<
            dyn futures::Stream<
                    Item = Result<friday_agent::AgentResponseChunk, friday_agent::AgentError>,
                > + 'static,
        >,
    > {
        todo!()
    }
}
