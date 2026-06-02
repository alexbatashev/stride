mod basic;

use std::sync::Arc;

use async_trait::async_trait;
use friday_agent::{AgentConfig, Tool, ToolDesc};
use llm::{Function, Tool as LlmTool};
use serde_json::{Value as JsonValue, json};

pub use basic::*;

use crate::vfs::MountedVfs;

pub struct VfsDocumentToMarkdownTool {
    pub fs: MountedVfs,
}

pub struct VfsMarkdownToPdfTool {
    pub fs: MountedVfs,
}

pub struct VfsMarkdownToOfficeWordTool {
    pub fs: MountedVfs,
}

pub struct VfsPresentationXmlToPptxTool {
    pub fs: MountedVfs,
    pub requires_confirmation: bool,
}

#[derive(ToolDesc)]
struct VfsDocumentToMarkdownParams {
    /// Absolute path to a PDF, DOC, DOCX, PPT, PPTX, XLS, or XLSX file, e.g. "/reports/report.pdf".
    path: String,
}

#[derive(ToolDesc)]
struct VfsMarkdownToPdfParams {
    /// Absolute path to write, e.g. "/reports/report.pdf".
    path: String,
    /// Markdown content to convert into a PDF document.
    content: String,
}

#[derive(ToolDesc)]
struct VfsMarkdownToOfficeWordParams {
    /// Absolute path to write, e.g. "/reports/report.docx".
    path: String,
    /// Markdown content to convert into a DOCX document.
    content: String,
}

#[derive(ToolDesc)]
struct VfsPresentationXmlToPptxParams {
    /// Absolute path to write, e.g. "/decks/strategy.pptx".
    path: String,
    /// Presentation XML to convert into a PPTX deck.
    content: String,
}

#[async_trait(?Send)]
impl Tool for VfsDocumentToMarkdownTool {
    fn name(&self) -> &str {
        "vfs_document_to_markdown"
    }

    fn readable_name(&self) -> &str {
        "Read Document"
    }

    fn definition(&self) -> LlmTool {
        LlmTool {
            r#type: llm::ToolType::Function,
            function: Function {
                name: self.name().to_owned(),
                description: "Read a PDF, Word, PowerPoint, or Excel file and convert it to Markdown. Use an absolute path such as /reports/file.pdf.".to_string(),
                parameters: Some(VfsDocumentToMarkdownParams::function_parameters()),
            },
        }
    }

    async fn execute(&self, _config: Arc<AgentConfig>, args: JsonValue) -> JsonValue {
        let params = match VfsDocumentToMarkdownParams::decode(args) {
            Ok(p) => p,
            Err(e) => return json!({"error": e}),
        };

        let data = match self.fs.read_bytes(&params.path).await {
            Ok((data, _)) => data,
            Err(e) => return json!({"error": e.to_string()}),
        };

        match fzilla::convert_to_markdown(data).await {
            Ok(content) => json!({"content": content}),
            Err(e) => json!({"error": e.to_string()}),
        }
    }
}

#[async_trait(?Send)]
impl Tool for VfsMarkdownToPdfTool {
    fn name(&self) -> &str {
        "vfs_markdown_to_pdf"
    }

    fn readable_name(&self) -> &str {
        "Write PDF"
    }

    fn definition(&self) -> LlmTool {
        LlmTool {
            r#type: llm::ToolType::Function,
            function: Function {
                name: self.name().to_owned(),
                description: "Convert Markdown to PDF and write it to a file path such as /reports/report.pdf.".to_string(),
                parameters: Some(VfsMarkdownToPdfParams::function_parameters()),
            },
        }
    }

    fn requires_confirmation(&self) -> bool {
        true
    }

    fn confirmation_prompt(&self, args: &JsonValue) -> String {
        let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("?");
        format!("Write PDF {path}")
    }

    async fn execute(&self, _config: Arc<AgentConfig>, args: JsonValue) -> JsonValue {
        let params = match VfsMarkdownToPdfParams::decode(args) {
            Ok(p) => p,
            Err(e) => return json!({"error": e}),
        };

        let data = match fzilla::markdown_to_pdf(params.content).await {
            Ok(data) => data,
            Err(e) => return json!({"error": e.to_string()}),
        };

        match self
            .fs
            .write_bytes(&params.path, &data, Some("application/pdf"))
            .await
        {
            Ok(()) => json!({"success": true, "path": params.path}),
            Err(e) => json!({"error": e.to_string()}),
        }
    }
}

#[async_trait(?Send)]
impl Tool for VfsMarkdownToOfficeWordTool {
    fn name(&self) -> &str {
        "vfs_markdown_to_office_word"
    }

    fn readable_name(&self) -> &str {
        "Write Word Document"
    }

    fn definition(&self) -> LlmTool {
        LlmTool {
            r#type: llm::ToolType::Function,
            function: Function {
                name: self.name().to_owned(),
                description: "Convert Markdown to DOCX and write it to a file path such as /reports/report.docx.".to_string(),
                parameters: Some(VfsMarkdownToOfficeWordParams::function_parameters()),
            },
        }
    }

    fn requires_confirmation(&self) -> bool {
        true
    }

    fn confirmation_prompt(&self, args: &JsonValue) -> String {
        let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("?");
        format!("Write Word document {path}")
    }

    async fn execute(&self, _config: Arc<AgentConfig>, args: JsonValue) -> JsonValue {
        let params = match VfsMarkdownToOfficeWordParams::decode(args) {
            Ok(p) => p,
            Err(e) => return json!({"error": e}),
        };

        let data = match fzilla::markdown_to_office_word(params.content).await {
            Ok(data) => data,
            Err(e) => return json!({"error": e.to_string()}),
        };

        match self
            .fs
            .write_bytes(
                &params.path,
                &data,
                Some("application/vnd.openxmlformats-officedocument.wordprocessingml.document"),
            )
            .await
        {
            Ok(()) => json!({"success": true, "path": params.path}),
            Err(e) => json!({"error": e.to_string()}),
        }
    }
}

#[async_trait(?Send)]
impl Tool for VfsPresentationXmlToPptxTool {
    fn name(&self) -> &str {
        "vfs_presentation_xml_to_pptx"
    }

    fn readable_name(&self) -> &str {
        "Write PowerPoint"
    }

    fn definition(&self) -> LlmTool {
        LlmTool {
            r#type: llm::ToolType::Function,
            function: Function {
                name: self.name().to_owned(),
                description: "Convert Friday presentation XML to PPTX and write it to a file path such as /decks/strategy.pptx. The XML root is <presentation size=\"wide|standard\">. Slides support layout=\"title|section|title-content|two-column|blank\", <title>, <subtitle>, <text>, <bullets><item>...</item></bullets>, <image src=\"/path.png\" x=\"in\" y=\"in\" w=\"in\" h=\"in\"/>, <textbox x=\"in\" y=\"in\" w=\"in\" h=\"in\">...</textbox>, <notes>, and <columns><left>...</left><right>...</right></columns> for two-column scientific slides.".to_string(),
                parameters: Some(VfsPresentationXmlToPptxParams::function_parameters()),
            },
        }
    }

    fn requires_confirmation(&self) -> bool {
        self.requires_confirmation
    }

    fn confirmation_prompt(&self, args: &JsonValue) -> String {
        let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("?");
        format!("Write PowerPoint presentation {path}")
    }

    async fn execute(&self, _config: Arc<AgentConfig>, args: JsonValue) -> JsonValue {
        let params = match VfsPresentationXmlToPptxParams::decode(args) {
            Ok(p) => p,
            Err(e) => return json!({"error": e}),
        };

        let mut assets = Vec::new();
        for path in match fzilla::presentation_xml_image_paths(&params.content) {
            Ok(paths) => paths,
            Err(e) => return json!({"error": e.to_string()}),
        } {
            let (data, mime_type) = match self.fs.read_bytes(&path).await {
                Ok(result) => result,
                Err(e) => return json!({"error": e.to_string()}),
            };
            assets.push(fzilla::PresentationImageAsset {
                path,
                data,
                mime_type,
            });
        }

        let data = match fzilla::presentation_xml_to_pptx_with_assets(params.content, assets).await
        {
            Ok(data) => data,
            Err(e) => return json!({"error": e.to_string()}),
        };

        match self
            .fs
            .write_bytes(
                &params.path,
                &data,
                Some("application/vnd.openxmlformats-officedocument.presentationml.presentation"),
            )
            .await
        {
            Ok(()) => json!({"success": true, "path": params.path}),
            Err(e) => json!({"error": e.to_string()}),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use friday_agent::{AgentConfig, ModelRegistry, Tool};
    use minisql::{ConnectionPool, Value};
    use serde_json::json;
    use uuid::Uuid;

    use super::*;
    use crate::vfs::{AnyFileProvider, LocalFileProvider, Vfs};
    use crate::db;

    async fn setup() -> (MountedVfs, Arc<Vfs>, Uuid, Arc<AgentConfig>) {
        let db = ConnectionPool::new("sqlite::memory:").unwrap();
        db.initialize_database(db::get_migrations()).await.unwrap();

        let owner = Uuid::now_v7();
        db.query_with_params(
            "INSERT INTO users (id, username, password_hash) VALUES (?, ?, ?)",
            vec![
                Value::Uuid(owner),
                Value::Text("alice".to_string()),
                Value::Text("hash".to_string()),
            ],
        )
        .await
        .unwrap();

        let base = tempfile::tempdir().unwrap().keep();
        let storage = AnyFileProvider::Local(LocalFileProvider::new(base).unwrap());
        let vfs = Arc::new(Vfs::new(db, storage, 3));
        let workspace_id = vfs
            .get_or_create_workspace(Uuid::now_v7(), None, owner)
            .await
            .unwrap();
        let config = Arc::new(AgentConfig {
            model_registry: ModelRegistry::default(),
            max_iterations: 1,
        });
        let fs = MountedVfs::new(vfs.clone(), workspace_id, owner);
        (fs, vfs, workspace_id, config)
    }

    #[tokio::test]
    async fn markdown_to_word_tool_writes_readable_document() {
        let (fs, _vfs, _ws, config) = setup().await;
        let write = VfsMarkdownToOfficeWordTool { fs: fs.clone() };
        let read = VfsDocumentToMarkdownTool { fs };

        let result = write
            .execute(
                config.clone(),
                json!({
                    "path": "/~workspace/reports/report.docx",
                    "content": "# Report\n\nHello **world**.",
                }),
            )
            .await;
        assert_eq!(result["success"], true);

        let result = read
            .execute(config, json!({"path": "/~workspace/reports/report.docx"}))
            .await;
        let content = result["content"].as_str().unwrap();
        assert!(content.contains("Report"));
        assert!(content.contains("Hello"));
    }

    #[tokio::test]
    async fn write_tool_rejects_global_path() {
        let (fs, _vfs, _ws, config) = setup().await;
        let write = VfsMarkdownToPdfTool { fs };
        let result = write
            .execute(config, json!({"path": "/reports/report.pdf", "content": "# x"}))
            .await;
        assert!(result["error"].as_str().unwrap().contains("read-only"));
    }

    #[tokio::test]
    async fn markdown_to_pdf_tool_writes_pdf_bytes() {
        let (fs, vfs, workspace_id, config) = setup().await;
        let write = VfsMarkdownToPdfTool { fs };

        let result = write
            .execute(
                config,
                json!({
                    "path": "/~workspace/reports/report.pdf",
                    "content": "# Report\n\nHello.",
                }),
            )
            .await;
        assert_eq!(result["success"], true);

        let (bytes, mime_type) = vfs
            .read_bytes(workspace_id, "reports/report.pdf")
            .await
            .unwrap();
        assert!(bytes.starts_with(b"%PDF-"));
        assert_eq!(mime_type.as_deref(), Some("application/pdf"));
    }

    #[tokio::test]
    async fn presentation_xml_tool_writes_readable_pptx() {
        let (fs, _vfs, _ws, config) = setup().await;
        fs.write_bytes(
            "/~workspace/figures/chart.png",
            &[137, 80, 78, 71, 13, 10, 26, 10],
            Some("image/png"),
        )
        .await
        .unwrap();

        let write = VfsPresentationXmlToPptxTool {
            fs: fs.clone(),
            requires_confirmation: false,
        };
        let read = VfsDocumentToMarkdownTool { fs };

        let result = write
            .execute(
                config.clone(),
                json!({
                    "path": "/~workspace/decks/roadmap.pptx",
                    "content": r#"<presentation size="wide"><slide layout="two-column"><title>Roadmap</title><columns><left><text>Next quarter</text><bullets><item>Documents</item><item>Presentations</item></bullets></left><right><image src="/~workspace/figures/chart.png" w="3" h="2"/></right></columns><notes>Explain chart source.</notes></slide></presentation>"#,
                }),
            )
            .await;
        assert_eq!(result["success"], true);

        let result = read
            .execute(config, json!({"path": "/~workspace/decks/roadmap.pptx"}))
            .await;
        let content = result["content"].as_str().unwrap();
        assert!(content.contains("Roadmap"));
        assert!(content.contains("Next quarter"));
        assert!(content.contains("Documents"));
        assert!(content.contains("Explain chart source"));
    }
}
