mod base;
pub mod mcp;
pub mod memory;
mod tool;
mod tool_registry;
pub mod tools;

extern crate self as friday_agent;

pub use agent_macro::ToolDesc;
pub use base::*;
pub use tool::*;
pub use tool_registry::*;

pub trait ToolDesc: Sized {
    fn function_parameters() -> llm::FunctionParameters;

    fn decode(value: serde_json::Value) -> Result<Self, String>;
}
