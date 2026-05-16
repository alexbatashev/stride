use crate::{AgentConfig, QuizQuestion, Tool, ToolDesc};
use async_trait::async_trait;
use llm::{Function, Tool as LlmTool};
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;

pub struct QuizTool;

#[derive(Deserialize)]
struct QuizOption {
    question: String,
    options: Vec<String>,
}

#[derive(ToolDesc)]
struct QuizParams {
    /// Questions to present to the user. Each item must be an object with a "question" string
    /// and an "options" array of suggested answer strings (may be empty for free-form answers).
    questions: Vec<QuizOption>,
}

#[async_trait(?Send)]
impl Tool for QuizTool {
    fn name(&self) -> &str {
        "quiz"
    }

    fn readable_name(&self) -> &str {
        "Quiz"
    }

    fn definition(&self) -> LlmTool {
        LlmTool {
            r#type: llm::ToolType::Function,
            function: Function {
                description:
                    "Present one or more questions to the user and collect their answers. \
                    Use this to resolve ambiguity, gather preferences, or quiz the user on a topic."
                        .to_string(),
                name: self.name().to_owned(),
                parameters: Some(QuizParams::function_parameters()),
            },
        }
    }

    async fn execute(&self, _config: Arc<AgentConfig>, _args: Value) -> Value {
        serde_json::json!({ "error": "quiz tool requires interactive agent handling" })
    }

    fn quiz_questions(&self, args: &Value) -> Option<Vec<QuizQuestion>> {
        let params = QuizParams::decode(args.clone()).ok()?;
        Some(
            params
                .questions
                .into_iter()
                .map(|q| QuizQuestion {
                    question: q.question,
                    options: q.options,
                })
                .collect(),
        )
    }
}
