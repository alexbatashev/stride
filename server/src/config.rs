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
    /// Whether this model accepts image inputs. When true, attached images are
    /// sent to the model and the `attach_image` tool is offered to the agent.
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

#[derive(Debug, Deserialize)]
pub struct Ldap {
    pub url: String,
    /// DN template for binding, e.g. "uid={username},ou=users,dc=example,dc=com"
    pub user_dn_template: String,
}

#[derive(Debug, Deserialize)]
pub struct Server {
    /// Full database connection URL. Takes precedence over `db_path`. Supports
    /// `sqlite://<path>`, `postgres://...`, and `postgresql://...`. May also be
    /// supplied via the `STRIDE_DATABASE_URL` environment variable.
    pub db_url: Option<String>,
    /// Filesystem path to a SQLite database. Used when `db_url` is unset; the
    /// server connects to `sqlite://<db_path>`.
    pub db_path: Option<String>,
    pub listen_addr: Option<String>,
    pub allow_registration: Option<bool>,
    pub ldap: Option<Ldap>,
    pub files: Option<Files>,
    pub telegram: Option<Telegram>,
    pub github: Option<GitHub>,
    pub google: Option<Google>,
    /// Public base URL the server is reachable at, e.g. `https://stride.example.com`.
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

/// Connection to the official, hosted GitHub MCP server. Linking an account uses
/// a standard GitHub OAuth App: the server redirects the user to GitHub, exchanges
/// the returned code for a user access token, and forwards that token to the MCP
/// server. Setting `client_id` and `client_secret` is all that is required to
/// activate the integration.
#[derive(Clone, Debug, Deserialize)]
pub struct GitHub {
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    /// OAuth scopes requested when linking an account. Defaults to a set covering
    /// the GitHub MCP server's common toolsets.
    pub scopes: Option<String>,
    /// Streamable HTTP endpoint of the GitHub MCP server. Defaults to the official
    /// hosted server; override only for GitHub Enterprise Cloud.
    pub mcp_url: Option<String>,
}

impl GitHub {
    pub fn read_client_id(&self) -> Option<String> {
        self.client_id
            .clone()
            .or_else(|| env::var("STRIDE_GITHUB_CLIENT_ID").ok())
            .filter(|value| !value.is_empty())
    }

    pub fn read_client_secret(&self) -> Option<String> {
        self.client_secret
            .clone()
            .or_else(|| env::var("STRIDE_GITHUB_CLIENT_SECRET").ok())
            .filter(|value| !value.is_empty())
    }

    pub fn scopes(&self) -> &str {
        self.scopes
            .as_deref()
            .filter(|value| !value.is_empty())
            .unwrap_or("repo read:org read:user")
    }

    pub fn mcp_url(&self) -> &str {
        self.mcp_url
            .as_deref()
            .filter(|value| !value.is_empty())
            .unwrap_or(crate::github::DEFAULT_MCP_URL)
    }

    /// Whether the OAuth App credentials needed to link accounts are present.
    pub fn is_configured(&self) -> bool {
        self.read_client_id().is_some() && self.read_client_secret().is_some()
    }
}

/// Google account linking via a standard OAuth 2.0 / OIDC client. Linking signs
/// the user in with Google, stores the resulting access and refresh tokens, and
/// forwards the (refreshed) access token to a Google MCP server. Unlike GitHub
/// there is no official hosted server, so `mcp_url` must point at a self-hosted
/// or third-party Google MCP server for any tools to be exposed.
#[derive(Clone, Debug, Deserialize)]
pub struct Google {
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    /// OAuth scopes requested when linking an account. Defaults to a set covering
    /// OIDC identity plus Calendar, Gmail, and Drive.
    pub scopes: Option<String>,
    /// Streamable HTTP endpoint of the Google MCP server. No default: without it
    /// no tools are exposed even when an account is linked.
    pub mcp_url: Option<String>,
}

impl Google {
    pub fn read_client_id(&self) -> Option<String> {
        self.client_id
            .clone()
            .or_else(|| env::var("STRIDE_GOOGLE_CLIENT_ID").ok())
            .filter(|value| !value.is_empty())
    }

    pub fn read_client_secret(&self) -> Option<String> {
        self.client_secret
            .clone()
            .or_else(|| env::var("STRIDE_GOOGLE_CLIENT_SECRET").ok())
            .filter(|value| !value.is_empty())
    }

    pub fn scopes(&self) -> &str {
        self.scopes
            .as_deref()
            .filter(|value| !value.is_empty())
            .unwrap_or(
                "openid email profile \
                 https://www.googleapis.com/auth/calendar \
                 https://www.googleapis.com/auth/gmail.modify \
                 https://www.googleapis.com/auth/drive",
            )
    }

    /// Configured MCP endpoint, if any. There is no default Google MCP server.
    pub fn mcp_url(&self) -> Option<&str> {
        self.mcp_url.as_deref().filter(|value| !value.is_empty())
    }

    /// Whether the OAuth client credentials needed to link accounts are present.
    pub fn is_configured(&self) -> bool {
        self.read_client_id().is_some() && self.read_client_secret().is_some()
    }
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
        if let Ok(url) = env::var("STRIDE_DATABASE_URL")
            && !url.is_empty()
        {
            return url;
        }

        let server = self.server.as_ref();

        if let Some(url) = server
            .and_then(|server| server.db_url.as_deref())
            .filter(|url| !url.is_empty())
        {
            return url.to_string();
        }

        let db_path = server
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
            .unwrap_or(false)
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
            .or_else(|| env::var(format!("STRIDE_{}_API_KEY", name.to_ascii_uppercase())).ok())
    }
}

impl McpServer {
    /// Request headers for this server, including a bearer `Authorization`
    /// header when a token is configured (or available via
    /// `STRIDE_MCP_<NAME>_TOKEN`).
    pub fn request_headers(&self, name: &str) -> Vec<(String, String)> {
        let mut headers: Vec<(String, String)> = self
            .headers
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        let token = self
            .token
            .clone()
            .or_else(|| env::var(format!("STRIDE_MCP_{}_TOKEN", name.to_ascii_uppercase())).ok());
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
            .or_else(|| env::var("STRIDE_BRAVE_API_KEY").ok())
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
            .or_else(|| env::var("STRIDE_TELEGRAM_BOT_API_KEY").ok())
    }

    pub fn read_bot_username(&self) -> Option<String> {
        self.bot_username
            .clone()
            .or_else(|| env::var("STRIDE_TELEGRAM_BOT_USERNAME").ok())
    }

    pub fn read_webhook_secret(&self) -> Option<String> {
        self.webhook_secret
            .clone()
            .or_else(|| env::var("STRIDE_TELEGRAM_WEBHOOK_SECRET").ok())
            .filter(|value| !value.is_empty())
    }

    pub fn read_webhook_url(&self) -> Option<String> {
        self.webhook_url
            .clone()
            .or_else(|| env::var("STRIDE_TELEGRAM_WEBHOOK_URL").ok())
            .filter(|value| !value.is_empty())
    }
}

impl Firecrawl {
    pub fn read_api_key(&self) -> Option<String> {
        self.api_key
            .clone()
            .or_else(|| env::var("STRIDE_FIRECRAWL_API_KEY").ok())
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
                db_url: None,
                db_path: Some("/tmp/stride-test.db".to_string()),
                listen_addr: Some("127.0.0.1:4000".to_string()),
                allow_registration: Some(false),
                ldap: None,
                files: None,
                telegram: None,
                github: None,
                google: None,
                public_url: None,
            }),
            tools: None,
            mcp: HashMap::new(),
        };

        assert_eq!(cfg.db_url(), "sqlite:///tmp/stride-test.db");
        assert_eq!(cfg.listen_addr(), "127.0.0.1:4000");
        assert!(!cfg.allow_registration());
    }

    #[test]
    fn db_url_takes_precedence_over_db_path() {
        let cfg: Config = toml::from_str(
            r#"
            providers = {}
            models = {}

            [server]
            db_path = "/tmp/ignored.db"
            db_url = "postgres://user:pass@localhost:5432/stride"
            "#,
        )
        .unwrap();

        assert_eq!(cfg.db_url(), "postgres://user:pass@localhost:5432/stride");
    }

    #[test]
    fn registration_is_disabled_by_default() {
        let cfg = Config {
            providers: HashMap::new(),
            models: HashMap::new(),
            server: None,
            tools: None,
            mcp: HashMap::new(),
        };

        assert!(!cfg.allow_registration());
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
            cache_dir = "/tmp/stride-execenv-test"
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
        assert_eq!(python.cache_dir.unwrap(), "/tmp/stride-execenv-test");
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
            bot_username = "stride_bot"
            webhook_secret = "secret"
            "#,
        )
        .unwrap();

        let telegram = cfg.server.unwrap().telegram.unwrap();
        assert_eq!(telegram.read_bot_api_key().unwrap(), "123:abc");
        assert_eq!(telegram.read_bot_username().unwrap(), "stride_bot");
        assert_eq!(telegram.webhook_secret.unwrap(), "secret");
    }

    #[test]
    fn github_config_loads() {
        let cfg: Config = toml::from_str(
            r#"
            providers = {}
            models = {}

            [server.github]
            client_id = "Iv1.abc"
            client_secret = "gh-secret"
            "#,
        )
        .unwrap();

        let github = cfg.server.unwrap().github.unwrap();
        assert_eq!(github.read_client_id().unwrap(), "Iv1.abc");
        assert_eq!(github.read_client_secret().unwrap(), "gh-secret");
        assert!(github.is_configured());
        assert_eq!(github.scopes(), "repo read:org read:user");
        assert_eq!(github.mcp_url(), "https://api.githubcopilot.com/mcp/");
    }

    #[test]
    fn google_config_loads() {
        let cfg: Config = toml::from_str(
            r#"
            providers = {}
            models = {}

            [server.google]
            client_id = "google-client-id"
            client_secret = "google-secret"
            mcp_url = "https://mcp.google.example.com/mcp"
            "#,
        )
        .unwrap();

        let google = cfg.server.unwrap().google.unwrap();
        assert_eq!(google.read_client_id().unwrap(), "google-client-id");
        assert_eq!(google.read_client_secret().unwrap(), "google-secret");
        assert!(google.is_configured());
        assert!(google.scopes().contains("auth/calendar"));
        assert!(google.scopes().contains("auth/gmail"));
        assert!(google.scopes().contains("auth/drive"));
        assert_eq!(google.mcp_url(), Some("https://mcp.google.example.com/mcp"));
    }

    #[test]
    fn google_without_mcp_url_has_no_endpoint() {
        let cfg: Config = toml::from_str(
            r#"
            providers = {}
            models = {}

            [server.google]
            client_id = "google-client-id"
            client_secret = "google-secret"
            "#,
        )
        .unwrap();

        let google = cfg.server.unwrap().google.unwrap();
        assert!(google.is_configured());
        assert_eq!(google.mcp_url(), None);
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
