use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelDesc {
    pub id: String,
    pub canonical_slug: Option<String>,
    pub created: Option<u64>,
    pub owned_by: Option<String>,
    pub context_length: Option<u32>,
    #[serde(default)]
    pub supported_parameters: Vec<String>,
    pub description: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelList<T = ModelDesc> {
    pub models: Vec<T>,
}

impl ModelDesc {
    pub fn get_name(&self) -> &str {
        match self.name.as_ref() {
            Some(name) => name.as_str(),
            None => self.id.as_str(),
        }
    }
}
