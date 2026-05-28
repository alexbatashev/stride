use std::sync::Arc;

use async_trait::async_trait;
use base64::{Engine, engine::general_purpose::STANDARD};
use llm::{Function, Tool as LlmTool};
use serde_json::{Value, json};

use crate::{AgentConfig, Tool, ToolDesc};

pub struct ConvertToMarkdownTool;
pub struct MarkdownToPdfTool;
pub struct MarkdownToOfficeWordTool;

#[derive(ToolDesc)]
struct ConvertToMarkdownParams {
    /// Base64-encoded PDF, DOC, DOCX, PPT, PPTX, XLS, or XLSX bytes.
    data_base64: String,
}

#[derive(ToolDesc)]
struct MarkdownToPdfParams {
    /// Markdown content to convert into a PDF document.
    content: String,
}

#[derive(ToolDesc)]
struct MarkdownToOfficeWordParams {
    /// Markdown content to convert into a DOCX document.
    content: String,
}

#[async_trait(?Send)]
impl Tool for ConvertToMarkdownTool {
    fn name(&self) -> &str {
        "convert_document_to_markdown"
    }

    fn readable_name(&self) -> &str {
        "Convert Document to Markdown"
    }

    fn definition(&self) -> LlmTool {
        LlmTool {
            r#type: llm::ToolType::Function,
            function: Function {
                name: self.name().to_owned(),
                description: "Convert PDF, Word, PowerPoint, or Excel bytes into Markdown."
                    .to_string(),
                parameters: Some(ConvertToMarkdownParams::function_parameters()),
            },
        }
    }

    async fn execute(&self, _config: Arc<AgentConfig>, args: Value) -> Value {
        let params = match ConvertToMarkdownParams::decode(args) {
            Ok(p) => p,
            Err(e) => return json!({"error": e}),
        };
        let data = match STANDARD.decode(params.data_base64) {
            Ok(data) => data,
            Err(e) => return json!({"error": e.to_string()}),
        };

        match fzilla::convert_to_markdown(data).await {
            Ok(content) => json!({"content": content}),
            Err(e) => json!({"error": e.to_string()}),
        }
    }
}

#[async_trait(?Send)]
impl Tool for MarkdownToPdfTool {
    fn name(&self) -> &str {
        "markdown_to_pdf"
    }

    fn readable_name(&self) -> &str {
        "Markdown to PDF"
    }

    fn definition(&self) -> LlmTool {
        LlmTool {
            r#type: llm::ToolType::Function,
            function: Function {
                name: self.name().to_owned(),
                description: "Convert Markdown content into base64-encoded PDF bytes.".to_string(),
                parameters: Some(MarkdownToPdfParams::function_parameters()),
            },
        }
    }

    async fn execute(&self, _config: Arc<AgentConfig>, args: Value) -> Value {
        let params = match MarkdownToPdfParams::decode(args) {
            Ok(p) => p,
            Err(e) => return json!({"error": e}),
        };

        match fzilla::markdown_to_pdf(params.content).await {
            Ok(data) => json!({
                "data_base64": STANDARD.encode(data),
                "mime_type": "application/pdf",
            }),
            Err(e) => json!({"error": e.to_string()}),
        }
    }
}

#[async_trait(?Send)]
impl Tool for MarkdownToOfficeWordTool {
    fn name(&self) -> &str {
        "markdown_to_office_word"
    }

    fn readable_name(&self) -> &str {
        "Markdown to Word"
    }

    fn definition(&self) -> LlmTool {
        LlmTool {
            r#type: llm::ToolType::Function,
            function: Function {
                name: self.name().to_owned(),
                description: "Convert Markdown content into base64-encoded DOCX bytes.".to_string(),
                parameters: Some(MarkdownToOfficeWordParams::function_parameters()),
            },
        }
    }

    async fn execute(&self, _config: Arc<AgentConfig>, args: Value) -> Value {
        let params = match MarkdownToOfficeWordParams::decode(args) {
            Ok(p) => p,
            Err(e) => return json!({"error": e}),
        };

        match fzilla::markdown_to_office_word(params.content).await {
            Ok(data) => json!({
                "data_base64": STANDARD.encode(data),
                "mime_type": "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            }),
            Err(e) => json!({"error": e.to_string()}),
        }
    }
}
