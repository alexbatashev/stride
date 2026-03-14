use super::ToolInvocation;
use crate::tools::ToolArg;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct ModelFunctionCall {
    pub(super) name: String,
    pub(super) arguments: String,
    #[serde(rename = "callID", skip_serializing_if = "Option::is_none")]
    pub(super) call_id: Option<String>,
}

pub(super) fn json_string<T: Serialize>(value: &T) -> Option<String> {
    serde_json::to_string(value).ok()
}

pub(super) fn tool_result_dictionary(
    call: &ModelFunctionCall,
    invocation: &ToolInvocation,
) -> HashMap<String, String> {
    let mut out = HashMap::from([
        ("name".to_owned(), call.name.clone()),
        ("status".to_owned(), invocation.status.as_str().to_owned()),
        (
            "result".to_owned(),
            invocation.result_json.clone().unwrap_or_default(),
        ),
    ]);
    if let Some(call_id) = &call.call_id {
        out.insert("callID".to_owned(), call_id.clone());
    }
    out
}

pub(super) fn extract_function_calls(raw: Option<&str>) -> Vec<ModelFunctionCall> {
    raw.and_then(|raw| serde_json::from_str::<Vec<ModelFunctionCall>>(raw).ok())
        .unwrap_or_default()
}

pub(super) fn parse_tool_args(arguments_json: &str) -> Vec<ToolArg> {
    let parsed = serde_json::from_str::<HashMap<String, serde_json::Value>>(arguments_json);
    let Ok(parsed) = parsed else {
        return vec![];
    };

    parsed
        .into_iter()
        .map(|(name, value)| {
            let value = match value {
                serde_json::Value::String(value) => value,
                _ => value.to_string(),
            };
            ToolArg { name, value }
        })
        .collect()
}
