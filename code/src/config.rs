use std::{collections::HashMap, env, fs, io, path::Path};

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub providers: HashMap<String, Provider>,
    pub models: HashMap<String, Model>,
}

#[derive(Debug, Deserialize)]
pub enum Kind {
    OpenAI,
    OpenRouter,
    Anthropic,
    Ollama,
}

#[derive(Debug, Deserialize)]
pub struct Provider {
    pub kind: Kind,
    pub url: String,
    token: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Model {
    pub slug: String,
    pub provider: String,
    /// Reasoning effort level requested from the model (`"low"`, `"medium"`,
    /// `"high"`). Takes precedence over the legacy `thinking` flag.
    pub reasoning_effort: Option<llm::ReasoningEffort>,
    /// Deprecated boolean toggle kept for backwards compatibility. `true` maps
    /// to medium effort when `reasoning_effort` is unset.
    pub thinking: Option<bool>,
    pub vision: Option<bool>,
}

impl Model {
    /// Resolves the effective reasoning effort, falling back to the legacy
    /// `thinking` flag (`true` -> medium) when `reasoning_effort` is unset.
    pub fn reasoning_effort(&self) -> Option<llm::ReasoningEffort> {
        self.reasoning_effort.or_else(|| {
            self.thinking
                .unwrap_or(true)
                .then_some(llm::ReasoningEffort::Medium)
        })
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, io::Error> {
        let string = fs::read_to_string(path)?;

        let cfg: Config = toml::from_str(&string).unwrap();

        Ok(cfg)
    }
}

impl Provider {
    pub fn read_token(&self, name: &str) -> Option<String> {
        self.token
            .clone()
            .or_else(|| env::var(format!("STRIDE_{}_API_KEY", name.to_ascii_uppercase())).ok())
    }
}
