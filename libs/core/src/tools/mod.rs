use std::collections::HashMap;

use crate::futures::BoxFuture;
use llm::{Function, FunctionParameters, FunctionProperty, Tool as LLMTool, ToolType};

use crate::js::JavaScriptRuntime;

#[derive(Debug, Clone)]
pub struct ToolArg {
    pub name: String,
    pub value: String,
}

pub trait Tool: Send + Sync {
    fn as_llm(&self) -> LLMTool;
    fn id(&self) -> &'static str;
    fn execute<'a>(&'a self, args: &'a [ToolArg]) -> BoxFuture<'a, String>;
}

#[derive(Debug, Default)]
pub struct JSTool;

impl JSTool {
    const DESCRIPTION: &'static str =
        "Execute ECMAScript code and return concatenated console.log output.";

    pub fn new() -> Self {
        Self
    }
}

impl Tool for JSTool {
    fn as_llm(&self) -> LLMTool {
        let mut properties = HashMap::new();
        properties.insert(
            "code".to_owned(),
            FunctionProperty {
                r#type: "string".to_owned(),
                description: "Valid ECMAScript JavaScript source code to execute.".to_owned(),
            },
        );
        properties.insert(
            "timeout".to_owned(),
            FunctionProperty {
                r#type: "number".to_owned(),
                description:
                    "Execution timeout in seconds. Must be greater than 0 and no longer than 180."
                        .to_owned(),
            },
        );

        LLMTool {
            r#type: ToolType::Function,
            function: Function {
                description: Self::DESCRIPTION.to_owned(),
                name: self.id().to_owned(),
                parameters: Some(vec![FunctionParameters {
                    r#type: "object".to_owned(),
                    properties,
                    required: Some(vec!["code".to_owned(), "timeout".to_owned()]),
                }]),
            },
        }
    }

    fn id(&self) -> &'static str {
        "execute_js"
    }

    fn execute<'a>(&'a self, args: &'a [ToolArg]) -> BoxFuture<'a, String> {
        BoxFuture::from_future(async move {
            let by_name: HashMap<&str, &str> = args
                .iter()
                .map(|arg| (arg.name.as_str(), arg.value.as_str()))
                .collect();

            let Some(code) = by_name.get("code").copied() else {
                return "Error: Missing required argument 'code'.".to_owned();
            };
            if code.is_empty() {
                return "Error: Missing required argument 'code'.".to_owned();
            }

            let Some(timeout_raw) = by_name.get("timeout").copied() else {
                return "Error: Missing or invalid required argument 'timeout'.".to_owned();
            };
            let Ok(timeout) = timeout_raw.parse::<i32>() else {
                return "Error: Missing or invalid required argument 'timeout'.".to_owned();
            };
            if timeout <= 0 {
                return "Error: 'timeout' must be greater than 0 seconds.".to_owned();
            }
            if timeout > 180 {
                return "Error: 'timeout' must not exceed 180 seconds.".to_owned();
            }

            let runtime = match JavaScriptRuntime::new() {
                Ok(runtime) => runtime,
                Err(error) => return format!("Error: {}", error),
            };
            let context = match runtime.make_context() {
                Ok(context) => context,
                Err(error) => return format!("Error: {}", error),
            };
            if let Err(error) = context.evaluate(code, "<execute_js>", 0, Some(timeout)) {
                return format!("Error: {}", error);
            }

            context.consume_console_output()
        })
    }
}
