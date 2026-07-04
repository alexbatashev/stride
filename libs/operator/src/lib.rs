use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use stride_agent::{
    AgentConfig, AgentResponseStream, BaseAgent, DEFAULT_MODEL, ModelRegEntry, ModelRegistry,
    tools::shell::{BashBackend, ShellTool},
};

#[cfg(feature = "ffi")]
uniffi::setup_scaffolding!();

#[cfg(feature = "ffi")]
mod ffi;

const SYSTEM_PROMPT: &str = "You are Stride Operator, a local macOS agent. Use the cloud only for LLM inference. Execute tools locally on this Mac and ask for approval before unsafe shell commands.";

static NEXT_THREAD_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperatorConfig {
    pub cloud_base_url: String,
    pub bearer_token: String,
    pub model: String,
    pub working_directory: Option<PathBuf>,
    pub max_iterations: usize,
}

impl OperatorConfig {
    pub fn authorized_endpoint(
        cloud_base_url: impl Into<String>,
        bearer_token: impl Into<String>,
    ) -> Self {
        Self {
            cloud_base_url: cloud_base_url.into(),
            bearer_token: bearer_token.into(),
            model: DEFAULT_MODEL.to_string(),
            working_directory: None,
            max_iterations: 90,
        }
    }

    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    pub fn working_directory(mut self, working_directory: impl Into<PathBuf>) -> Self {
        self.working_directory = Some(working_directory.into());
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperatorThreadSummary {
    pub id: String,
    pub title: String,
}

pub struct Operator {
    config: Arc<AgentConfig>,
    model: String,
    working_directory: Option<PathBuf>,
}

impl Operator {
    pub fn new(config: OperatorConfig) -> Self {
        let mut model_registry = ModelRegistry::new();
        model_registry.add_model(
            DEFAULT_MODEL,
            ModelRegEntry::openai_compatible(
                &config.cloud_base_url,
                config.bearer_token,
                config.model,
            ),
        );

        Self {
            config: Arc::new(AgentConfig {
                model_registry,
                max_iterations: config.max_iterations,
            }),
            model: DEFAULT_MODEL.to_string(),
            working_directory: config.working_directory,
        }
    }

    pub fn new_thread(&self) -> OperatorThread {
        let mut agent = BaseAgent::new(
            self.model.clone(),
            self.config.clone(),
            system_prompt(self.working_directory.as_ref()),
            Vec::new(),
        );
        register_local_tools(&mut agent);

        OperatorThread {
            id: next_thread_id(),
            title: "New local thread".to_string(),
            agent,
        }
    }
}

pub struct OperatorThread {
    id: String,
    title: String,
    agent: BaseAgent,
}

impl OperatorThread {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn summary(&self) -> OperatorThreadSummary {
        OperatorThreadSummary {
            id: self.id.clone(),
            title: self.title.clone(),
        }
    }

    pub fn tool_names(&self) -> Vec<String> {
        self.agent
            .tool_definitions()
            .into_iter()
            .map(|tool| tool.function.name)
            .collect()
    }

    pub async fn make_turn(&self, content: impl Into<String>) -> AgentResponseStream {
        self.agent.make_turn(content.into(), Vec::new()).await
    }
}

fn register_local_tools(agent: &mut BaseAgent) {
    agent.register_tool(ShellTool::new(BashBackend));
}

fn system_prompt(working_directory: Option<&PathBuf>) -> String {
    let mut prompt = SYSTEM_PROMPT.to_string();
    if let Some(working_directory) = working_directory {
        prompt.push_str(&format!(
            "\nDefault working directory: {}",
            working_directory.display()
        ));
    }
    prompt
}

fn next_thread_id() -> String {
    format!("local:{}", NEXT_THREAD_ID.fetch_add(1, Ordering::Relaxed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_threads_expose_real_shell_tool() {
        let operator = Operator::new(OperatorConfig::authorized_endpoint(
            "http://127.0.0.1:3000",
            "token",
        ));
        let thread = operator.new_thread();

        assert!(thread.tool_names().contains(&"shell".to_string()));
    }

    #[test]
    fn config_defaults_to_authorized_cloud_endpoint() {
        let config = OperatorConfig::authorized_endpoint("http://127.0.0.1:3000/v1", "token");

        assert_eq!(config.cloud_base_url, "http://127.0.0.1:3000/v1");
        assert_eq!(config.bearer_token, "token");
        assert_eq!(config.model, DEFAULT_MODEL);
        assert_eq!(config.max_iterations, 90);
    }
}
