use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub enum ToolType {
    #[default]
    #[serde(rename = "function")]
    Function,
}

#[derive(Clone, Default, Serialize, Debug, Deserialize)]
pub struct Tool {
    pub r#type: ToolType,
    pub function: Function,
}

#[derive(Clone, Default, Serialize, Debug, Deserialize)]
pub struct Function {
    pub description: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<Vec<FunctionParameters>>,
}

#[derive(Clone, Default, Serialize, Debug, Deserialize)]
pub struct FunctionParameters {
    pub r#type: String,
    pub properties: HashMap<String, FunctionProperty>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<Vec<String>>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct FunctionProperty {
    pub r#type: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolChoice {
    Unnamed(UnnamedToolChoice),
    Named {
        r#type: String,
        function: FunctionRef,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UnnamedToolChoice {
    None,
    Auto,
    Required,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionRef {
    name: String,
}

impl FunctionRef {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}
