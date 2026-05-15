use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use llm::{Function, Tool as LlmTool};
use serde_json::{Value, json};

use crate::{AgentConfig, Tool, ToolDesc};

pub(crate) struct SearchEntry {
    pub name: String,
    pub description: String,
    pub definition: LlmTool,
}

pub struct SearchTool {
    pub(crate) entries: Arc<Mutex<Vec<SearchEntry>>>,
    pub(crate) slot: Arc<Mutex<Vec<LlmTool>>>,
}

#[derive(ToolDesc)]
struct SearchToolsParams {
    /// Keywords to search for among available tools.
    query: String,
}

#[async_trait(?Send)]
impl Tool for SearchTool {
    fn name(&self) -> &str {
        "search_tools"
    }

    fn readable_name(&self) -> &str {
        "Search Tools"
    }

    fn definition(&self) -> LlmTool {
        LlmTool {
            r#type: llm::ToolType::Function,
            function: Function {
                description: "Search for additional tools by keyword. Matching tools will be available on the next turn only.".to_string(),
                name: self.name().to_owned(),
                parameters: Some(SearchToolsParams::function_parameters()),
            },
        }
    }

    async fn execute(&self, _config: Arc<AgentConfig>, args: Value) -> Value {
        let query = match SearchToolsParams::decode(args) {
            Ok(p) => p.query.to_lowercase(),
            Err(e) => return json!({ "error": e }),
        };

        let entries = self.entries.lock().unwrap();
        let matches: Vec<&SearchEntry> = entries
            .iter()
            .filter(|e| {
                e.name.to_lowercase().contains(&query)
                    || e.description.to_lowercase().contains(&query)
            })
            .collect();

        let names: Vec<&str> = matches.iter().map(|e| e.name.as_str()).collect();
        *self.slot.lock().unwrap() = matches.iter().map(|e| e.definition.clone()).collect();

        json!({ "found": names.len(), "tools": names })
    }
}
