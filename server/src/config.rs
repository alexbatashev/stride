use std::{collections::HashMap, env, fs, io, path::Path};

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub providers: HashMap<String, Provider>,
    pub models: HashMap<String, Model>,
    pub server: Option<Server>,
    pub tools: Option<Tools>,
    #[serde(default)]
    pub mcp: HashMap<String, McpServer>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct McpServer {
    pub url: String,
    token: Option<String>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
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
    /// Whether this model accepts image inputs. When true, attached images are
    /// sent to the model and the `attach_image` tool is offered to the agent.
    pub vision: Option<bool>,
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
    pub allow_registration: Option<bool>,
    pub ldap: Option<Ldap>,
    pub files: Option<Files>,
    pub telegram: Option<Telegram>,
    /// Public base URL the server is reachable at, e.g. `https://friday.example.com`.
    /// Used to build capability URLs for image attachments served to vision
    /// models. When unset, images are sent inline as base64 instead.
    pub public_url: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Telegram {
    pub bot_api_key: Option<String>,
    pub bot_username: Option<String>,
    pub webhook_secret: Option<String>,
    /// Public HTTPS URL Telegram posts updates to, e.g. `https://host/api/telegram/webhook`. When
    /// set with a bot token, the server registers it on startup with `callback_query` updates
    /// enabled, so inline button taps are delivered.
    pub webhook_url: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Tools {
    pub web_search: Option<WebSearch>,
    pub firecrawl: Option<Firecrawl>,
    pub python: Option<Python>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct WebSearch {
    pub searxng_endpoint: String,
    pub searxng_request_delay_seconds: Option<u64>,
    pub brave_api_key: Option<String>,
    pub brave_endpoint: Option<String>,
    pub include_arxiv: Option<bool>,
    pub include_pubmed: Option<bool>,
    pub include_uspto: Option<bool>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Firecrawl {
    pub api_key: Option<String>,
    pub api_url: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Python {
    pub enabled: Option<bool>,
    pub cache_dir: Option<String>,
    pub backend: Option<PythonBackend>,
    pub threads: Option<usize>,
    pub preinit: Option<bool>,
    pub max_runtime_seconds: Option<u64>,
    pub max_memory_bytes: Option<u64>,
    pub max_cpu_fuel: Option<u64>,
    pub network: Option<PythonNetwork>,
}

#[derive(Clone, Debug, Deserialize)]
pub enum PythonBackend {
    Mock,
    Eryx,
}

#[derive(Clone, Debug, Deserialize)]
pub enum PythonNetwork {
    Blocked,
    Allowed,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Files {
    pub keep_versions: Option<usize>,
    pub local: Option<LocalFiles>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct LocalFiles {
    pub enabled: bool,
    pub base: String,
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

    pub fn allow_registration(&self) -> bool {
        self.server
            .as_ref()
            .and_then(|server| server.allow_registration)
            .unwrap_or(true)
    }

    /// Public base URL (without trailing slash) used to build externally
    /// reachable links such as image capability URLs.
    pub fn public_url(&self) -> Option<String> {
        self.server
            .as_ref()
            .and_then(|server| server.public_url.as_deref())
            .map(|url| url.trim_end_matches('/').to_string())
    }
}

impl Provider {
    pub fn read_token(&self, name: &str) -> Option<String> {
        self.token
            .clone()
            .or_else(|| env::var(format!("FRIDAY_{}_API_KEY", name.to_ascii_uppercase())).ok())
    }
}

impl McpServer {
    /// Request headers for this server, including a bearer `Authorization`
    /// header when a token is configured (or available via
    /// `FRIDAY_MCP_<NAME>_TOKEN`).
    pub fn request_headers(&self, name: &str) -> Vec<(String, String)> {
        let mut headers: Vec<(String, String)> = self
            .headers
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        let token = self
            .token
            .clone()
            .or_else(|| env::var(format!("FRIDAY_MCP_{}_TOKEN", name.to_ascii_uppercase())).ok());
        if let Some(token) = token {
            headers.push(("Authorization".to_string(), format!("Bearer {token}")));
        }

        headers
    }
}

impl WebSearch {
    pub fn read_brave_api_key(&self) -> Option<String> {
        self.brave_api_key
            .clone()
            .or_else(|| env::var("FRIDAY_BRAVE_API_KEY").ok())
    }

    pub fn brave_endpoint(&self) -> &str {
        self.brave_endpoint
            .as_deref()
            .unwrap_or("https://api.search.brave.com/res/v1/web/search")
    }
}

impl Telegram {
    pub fn read_bot_api_key(&self) -> Option<String> {
        self.bot_api_key
            .clone()
            .or_else(|| env::var("FRIDAY_TELEGRAM_BOT_API_KEY").ok())
    }

    pub fn read_bot_username(&self) -> Option<String> {
        self.bot_username
            .clone()
            .or_else(|| env::var("FRIDAY_TELEGRAM_BOT_USERNAME").ok())
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
                allow_registration: Some(false),
                ldap: None,
                files: None,
                telegram: None,
                public_url: None,
            }),
            tools: None,
            mcp: HashMap::new(),
        };

        assert_eq!(cfg.db_url(), "sqlite:///tmp/friday-test.db");
        assert_eq!(cfg.listen_addr(), "127.0.0.1:4000");
        assert!(!cfg.allow_registration());
    }

    #[test]
    fn registration_is_enabled_by_default() {
        let cfg = Config {
            providers: HashMap::new(),
            models: HashMap::new(),
            server: None,
            tools: None,
            mcp: HashMap::new(),
        };

        assert!(cfg.allow_registration());
    }

    #[test]
    fn tool_parameters_load_from_config() {
        let cfg: Config = toml::from_str(
            r#"
            providers = {}
            models = {}

            [tools.web_search]
            searxng_endpoint = "https://search.example.com"
            searxng_request_delay_seconds = 2
            brave_api_key = "brave-test"

            [tools.firecrawl]
            api_key = "fc-test"
            api_url = "https://firecrawl.example.com"
            "#,
        )
        .unwrap();

        let tools = cfg.tools.unwrap();
        let web_search = tools.web_search.unwrap();
        assert_eq!(web_search.searxng_endpoint, "https://search.example.com");
        assert_eq!(web_search.searxng_request_delay_seconds, Some(2));
        assert_eq!(web_search.read_brave_api_key().unwrap(), "brave-test");
        assert_eq!(
            web_search.brave_endpoint(),
            "https://api.search.brave.com/res/v1/web/search"
        );

        let firecrawl = tools.firecrawl.unwrap();
        assert_eq!(firecrawl.read_api_key().unwrap(), "fc-test");
        assert_eq!(firecrawl.api_url(), "https://firecrawl.example.com");
    }

    #[test]
    fn python_tool_parameters_load_from_config() {
        let cfg: Config = toml::from_str(
            r#"
            providers = {}
            models = {}

            [tools.python]
            enabled = true
            cache_dir = "/tmp/friday-execenv-test"
            backend = "Eryx"
            threads = 4
            preinit = true
            max_runtime_seconds = 15
            max_memory_bytes = 67108864
            max_cpu_fuel = 1000
            network = "Blocked"
            "#,
        )
        .unwrap();

        let python = cfg.tools.unwrap().python.unwrap();
        assert_eq!(python.cache_dir.unwrap(), "/tmp/friday-execenv-test");
        assert!(matches!(python.backend.unwrap(), PythonBackend::Eryx));
        assert_eq!(python.threads, Some(4));
        assert_eq!(python.max_runtime_seconds, Some(15));
        assert_eq!(python.max_memory_bytes, Some(67_108_864));
        assert_eq!(python.max_cpu_fuel, Some(1000));
        assert!(matches!(python.network.unwrap(), PythonNetwork::Blocked));
    }

    #[test]
    fn telegram_config_loads() {
        let cfg: Config = toml::from_str(
            r#"
            providers = {}
            models = {}

            [server.telegram]
            bot_api_key = "123:abc"
            bot_username = "friday_bot"
            webhook_secret = "secret"
            "#,
        )
        .unwrap();

        let telegram = cfg.server.unwrap().telegram.unwrap();
        assert_eq!(telegram.read_bot_api_key().unwrap(), "123:abc");
        assert_eq!(telegram.read_bot_username().unwrap(), "friday_bot");
        assert_eq!(telegram.webhook_secret.unwrap(), "secret");
    }

    #[test]
    fn example_config_loads() {
        let cfg: Config = toml::from_str(include_str!("../config.toml.example")).unwrap();

        assert!(cfg.providers.contains_key("openai"));
        assert!(cfg.models.contains_key("gpt_4_1"));
        assert!(cfg.server.unwrap().ldap.is_some());
        assert!(cfg.tools.unwrap().web_search.is_some());
        assert!(cfg.mcp.contains_key("deepwiki"));
    }

    #[test]
    fn mcp_servers_load_and_build_headers() {
        let cfg: Config = toml::from_str(
            r#"
            providers = {}
            models = {}

            [mcp.internal]
            url = "https://mcp.example.com/mcp"
            token = "secret-token"
            headers = { X-Tenant = "acme" }
            "#,
        )
        .unwrap();

        let server = cfg.mcp.get("internal").unwrap();
        assert_eq!(server.url, "https://mcp.example.com/mcp");

        let headers = server.request_headers("internal");
        assert!(headers.contains(&("X-Tenant".to_string(), "acme".to_string())));
        assert!(headers.contains(&(
            "Authorization".to_string(),
            "Bearer secret-token".to_string()
        )));
    }
}
