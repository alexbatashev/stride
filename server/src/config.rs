use std::{collections::HashMap, env, fs, io, path::Path};

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub providers: HashMap<String, Provider>,
    pub models: HashMap<String, Model>,
    pub server: Option<Server>,
    pub tools: Option<Tools>,
}

#[derive(Debug, Deserialize)]
pub enum Kind {
    OpenAI,
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
    pub thinking: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct Ldap {
    pub url: String,
    /// DN template for binding, e.g. "uid={username},ou=users,dc=example,dc=com"
    pub user_dn_template: String,
}

#[derive(Debug, Deserialize)]
pub struct Server {
    pub db_path: Option<String>,
    pub listen_addr: Option<String>,
    pub ldap: Option<Ldap>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Tools {
    pub web_search: Option<WebSearch>,
    pub firecrawl: Option<Firecrawl>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct WebSearch {
    pub searxng_endpoint: String,
    pub include_arxiv: Option<bool>,
    pub include_pubmed: Option<bool>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Firecrawl {
    pub api_key: Option<String>,
    pub api_url: Option<String>,
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, io::Error> {
        let string = fs::read_to_string(path)?;

        let cfg: Config = toml::from_str(&string)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;

        Ok(cfg)
    }

    pub fn db_url(&self) -> String {
        let db_path = self
            .server
            .as_ref()
            .and_then(|server| server.db_path.as_deref())
            .unwrap_or("/tmp/server.db");

        format!("sqlite://{db_path}")
    }

    pub fn listen_addr(&self) -> &str {
        self.server
            .as_ref()
            .and_then(|server| server.listen_addr.as_deref())
            .unwrap_or("0.0.0.0:3000")
    }
}

impl Provider {
    pub fn read_token(&self, name: &str) -> Option<String> {
        self.token
            .clone()
            .or_else(|| env::var(format!("FRIDAY_{}_API_KEY", name.to_ascii_uppercase())).ok())
    }
}

impl Firecrawl {
    pub fn read_api_key(&self) -> Option<String> {
        self.api_key
            .clone()
            .or_else(|| env::var("FRIDAY_FIRECRAWL_API_KEY").ok())
    }

    pub fn api_url(&self) -> &str {
        self.api_url
            .as_deref()
            .unwrap_or("https://api.firecrawl.dev")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_parameters_use_config_values() {
        let cfg = Config {
            providers: HashMap::new(),
            models: HashMap::new(),
            server: Some(Server {
                db_path: Some("/tmp/friday-test.db".to_string()),
                listen_addr: Some("127.0.0.1:4000".to_string()),
                ldap: None,
            }),
            tools: None,
        };

        assert_eq!(cfg.db_url(), "sqlite:///tmp/friday-test.db");
        assert_eq!(cfg.listen_addr(), "127.0.0.1:4000");
    }

    #[test]
    fn tool_parameters_load_from_config() {
        let cfg: Config = toml::from_str(
            r#"
            providers = {}
            models = {}

            [tools.web_search]
            searxng_endpoint = "https://search.example.com"

            [tools.firecrawl]
            api_key = "fc-test"
            api_url = "https://firecrawl.example.com"
            "#,
        )
        .unwrap();

        let tools = cfg.tools.unwrap();
        assert_eq!(
            tools.web_search.unwrap().searxng_endpoint,
            "https://search.example.com"
        );

        let firecrawl = tools.firecrawl.unwrap();
        assert_eq!(firecrawl.read_api_key().unwrap(), "fc-test");
        assert_eq!(firecrawl.api_url(), "https://firecrawl.example.com");
    }
}
