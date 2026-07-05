use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use llm::{Function, Tool as LlmTool};
use serde_json::{Value, json};

use crate::{AgentConfig, Tool, ToolDesc};

pub(crate) struct SearchEntry {
    pub name: String,
    pub description: String,
    pub definition: LlmTool,
    pub group: Option<String>,
}

pub struct SearchTool {
    pub(crate) entries: Arc<Mutex<Vec<SearchEntry>>>,
    pub(crate) slot: Arc<Mutex<Vec<LlmTool>>>,
    pub(crate) preview_limit: Arc<Mutex<usize>>,
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
                description: self.description(),
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

impl SearchTool {
    fn description(&self) -> String {
        let entries = self.entries.lock().unwrap();
        let mut description = "Search for additional tools by keyword. Matching tools will be available on the next turn only.".to_string();

        if entries.is_empty() {
            return description;
        }

        description.push_str(&format!(
            " {} additional tools are available through this search.",
            entries.len()
        ));

        let preview_limit = *self.preview_limit.lock().unwrap();
        let standalone: Vec<&str> = entries
            .iter()
            .filter(|entry| entry.group.is_none())
            .map(|entry| entry.name.as_str())
            .take(preview_limit)
            .collect();
        if !standalone.is_empty() {
            description.push_str(" Tool names include: ");
            description.push_str(&standalone.join(", "));
            let standalone_count = entries.iter().filter(|entry| entry.group.is_none()).count();
            if standalone_count > standalone.len() {
                description.push_str(&format!(
                    " and {} more",
                    standalone_count - standalone.len()
                ));
            }
            description.push('.');
        }

        let mut groups = BTreeMap::<String, usize>::new();
        for entry in entries.iter().filter_map(|entry| entry.group.as_ref()) {
            *groups.entry(entry.clone()).or_default() += 1;
        }
        if !groups.is_empty() {
            let groups: Vec<String> = groups
                .into_iter()
                .map(|(group, count)| {
                    let suffix = if count == 1 { "tool" } else { "tools" };
                    format!("{group} ({count} {suffix})")
                })
                .collect();
            description.push_str(" MCP servers include: ");
            description.push_str(&groups.join(", "));
            description.push('.');
        }

        description
    }
}
