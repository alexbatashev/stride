use serde::Deserialize;
use std::env;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Failed to read config file: {0}")]
    ReadError(#[from] std::io::Error),
    #[error("Failed to parse TOML: {0}")]
    ParseError(#[from] toml::de::Error),
    #[error("Provider '{0}' not found")]
    ProviderNotFound(String),
    #[error("Environment variable '{0}' not set")]
    EnvVarNotSet(String),
    #[error("No providers configured")]
    NoProviders,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub default: DefaultConfig,
    pub providers: Vec<ProviderConfig>,
    #[serde(default)]
    pub agent: AgentConfig,
    #[serde(default)]
    pub daemon: DaemonConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DefaultConfig {
    pub provider: String,
    pub model: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ProviderConfig {
    pub name: String,
    #[serde(rename = "type")]
    pub provider_type: ProviderType,
    pub base_url: String,
    pub api_key: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "lowercase")]
pub enum ProviderType {
    #[serde(alias = "openai")]
    OpenAi,
    #[serde(alias = "anthropic")]
    Anthropic,
    #[serde(alias = "ollama")]
    Ollama,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct AgentConfig {
    #[serde(default = "default_max_iterations")]
    pub max_iterations: usize,
    #[serde(default = "default_confirm_destructive")]
    pub confirm_destructive: bool,
    #[serde(default)]
    pub thinking: Option<ThinkingConfig>,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct DaemonConfig {
    #[serde(default)]
    pub database_path: Option<String>,
    #[serde(default)]
    pub socket_path: Option<String>,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct ThinkingConfig {
    #[serde(default = "default_thinking_type")]
    pub thinking_type: String,
    #[serde(default)]
    pub budget_tokens: Option<u32>,
}

fn default_max_iterations() -> usize {
    50
}

fn default_confirm_destructive() -> bool {
    true
}

fn default_thinking_type() -> String {
    "native".to_string()
}

impl Config {
    /// Load configuration from a file path
    pub fn from_file(path: &str) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path)?;
        Self::from_str(&content)
    }

    /// Load configuration from a string
    pub fn from_str(content: &str) -> Result<Self, ConfigError> {
        let mut config: Config = toml::from_str(content)?;
        config.resolve_env_vars()?;
        Ok(config)
    }

    /// Get a provider configuration by name
    pub fn get_provider(&self, name: &str) -> Result<&ProviderConfig, ConfigError> {
        self.providers
            .iter()
            .find(|p| p.name == name)
            .ok_or_else(|| ConfigError::ProviderNotFound(name.to_string()))
    }

    /// Get the default provider configuration
    pub fn get_default_provider(&self) -> Result<&ProviderConfig, ConfigError> {
        if self.providers.is_empty() {
            return Err(ConfigError::NoProviders);
        }
        self.get_provider(&self.default.provider)
    }

    /// Resolve environment variable references in api_key fields
    fn resolve_env_vars(&mut self) -> Result<(), ConfigError> {
        for i in 0..self.providers.len() {
            if let Some(api_key) = &self.providers[i].api_key {
                let expanded = Self::expand_env_vars(api_key)?;
                self.providers[i].api_key = Some(expanded);
            }
        }
        Ok(())
    }

    /// Expand environment variables in a string
    fn expand_env_vars(input: &str) -> Result<String, ConfigError> {
        let mut result = input.to_string();

        // Handle ${VAR_NAME} syntax
        loop {
            let Some(start) = result.find("${") else {
                break;
            };
            let Some(end_offset) = result[start..].find('}') else {
                break;
            };
            let end = start + end_offset;

            let var_name = &result[start + 2..end];
            let var_value =
                env::var(var_name).map_err(|_| ConfigError::EnvVarNotSet(var_name.to_string()))?;

            result.replace_range(start..=end, &var_value);
        }

        // Handle $VAR_NAME syntax (simpler, no braces)
        let mut final_result = String::new();
        let mut chars = result.chars().peekable();

        while let Some(ch) = chars.next() {
            if ch == '$' && chars.peek() != Some(&'{') {
                let mut var_name = String::new();
                while let Some(&ch) = chars.peek() {
                    if ch.is_alphanumeric() || ch == '_' {
                        var_name.push(ch);
                        chars.next();
                    } else {
                        break;
                    }
                }
                if !var_name.is_empty() {
                    let var_value = env::var(&var_name)
                        .map_err(|_| ConfigError::EnvVarNotSet(var_name.clone()))?;
                    final_result.push_str(&var_value);
                } else {
                    final_result.push('$');
                }
            } else {
                final_result.push(ch);
            }
        }

        Ok(final_result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_config_parsing() {
        let toml_str = r#"
[default]
provider = "anthropic"
model = "claude-sonnet-4-20250514"

[[providers]]
name = "anthropic"
type = "anthropic"
base_url = "https://api.anthropic.com"
api_key = "test-key"
"#;

        let config = Config::from_str(toml_str).unwrap();
        assert_eq!(config.default.provider, "anthropic");
        assert_eq!(config.default.model, "claude-sonnet-4-20250514");
        assert_eq!(config.providers.len(), 1);
    }

    #[test]
    fn test_env_var_expansion() {
        unsafe { env::set_var("TEST_API_KEY", "secret123") };

        let toml_str = r#"
[default]
provider = "anthropic"
model = "claude-sonnet-4-20250514"

[[providers]]
name = "anthropic"
type = "anthropic"
base_url = "https://api.anthropic.com"
api_key = "${TEST_API_KEY}"
"#;

        let config = Config::from_str(toml_str).unwrap();
        assert_eq!(config.providers[0].api_key, Some("secret123".to_string()));
    }
}
