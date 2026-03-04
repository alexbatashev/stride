use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelResponse {
    pub id: String,
    pub canonical_slug: Option<String>,
    pub created: u64,
    pub owned_by: Option<String>,
    pub context_length: Option<u32>,
    pub supported_parameters: Vec<String>,
    pub description: Option<String>,
    name: Option<String>,
}

impl ModelResponse {
    pub fn get_name(&self) -> &str {
        match self.name.as_ref() {
            Some(name) => name.as_str(),
            None => self.id.as_str(),
        }
    }
}
