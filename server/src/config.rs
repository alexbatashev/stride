use std::{collections::HashMap, env, fs, io, path::Path};

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub providers: HashMap<String, Provider>,
    pub models: HashMap<String, Model>,
    pub server: Option<Server>,
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
pub struct Server {
    pub db_path: Option<String>,
    pub listen_addr: Option<String>,
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
            .or_else(|| env::var(&format!("FRIDAY_{}_API_KEY", name.to_ascii_uppercase())).ok())
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
            }),
        };

        assert_eq!(cfg.db_url(), "sqlite:///tmp/friday-test.db");
        assert_eq!(cfg.listen_addr(), "127.0.0.1:4000");
    }
}
