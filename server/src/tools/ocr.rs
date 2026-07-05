use std::sync::Arc;

use async_trait::async_trait;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use execenv::ExecutionService;
use llm::{CompletionRequest, Function, ImageSource, Message, Role, Tool as LlmTool};
use serde::Deserialize;
use serde_json::{Value as JsonValue, json};
use stride_agent::{AgentConfig, DEFAULT_MODEL, ModelRegEntry, Tool, ToolDesc};

use crate::vfs::MountedVfs;

/// Registry key for the specialized OCR model. Resolved from the model registry;
/// falls back to the default model when unset. Kept out of the user-facing chat
/// model list via `INTERNAL_MODEL_KEYS`.
pub const OCR_MODEL: &str = "ocr";

/// Upper bound on the number of pages processed from a single PDF. Also enforced
/// in the preprocessing script so oversized documents never blow up stdout.
const MAX_PAGES: usize = 25;

/// How much of the extracted Markdown to echo back inline as a preview.
const PREVIEW_CHARS: usize = 1500;

const SYSTEM_PROMPT: &str = "You are an OCR engine. Transcribe the document image into clean, \
well-structured GitHub-flavored Markdown. Preserve headings, lists, and tables (use Markdown \
table syntax) and keep the original reading order. Transcribe text verbatim in its original \
language; do not translate, summarize, or add commentary. Do not wrap the whole output in a code \
fence. For a region that is a photo or illustration with no text, omit it or note it briefly in \
italics.";

/// Extracts text from documents. Digital PDFs with a real text layer are read
/// directly (no model call); scanned PDFs and images are transcribed with a
/// vision model. The result is written to the workspace as Markdown.
pub struct OcrTool {
    pub fs: MountedVfs,
    /// Shared Python interpreter, used to read PDFs. `None` when the Python tool
    /// is disabled, in which case only image inputs are supported.
    pub python: Option<Arc<dyn ExecutionService>>,
    /// Absolute guest path of the writable area (e.g. `/~workspace`) where the
    /// default output file is written.
    pub writable_root: String,
}

#[derive(ToolDesc)]
struct OcrParams {
    /// Absolute path to the document in the file system: an image (png, jpg,
    /// webp, tiff) or a PDF, e.g. "/~workspace/scan.pdf" or "/Invoices/bill.jpg".
    path: String,
    /// Optional absolute path to write the Markdown to. Defaults to
    /// "<writable-root>/ocr/<name>.md".
    output_path: Option<String>,
}

#[derive(Deserialize)]
#[serde(tag = "kind")]
enum PdfResult {
    #[serde(rename = "text")]
    Text { pages: usize, text: Vec<String> },
    #[serde(rename = "images")]
    Images {
        pages: usize,
        images: Vec<PageImage>,
        empty_pages: Vec<usize>,
    },
    #[serde(rename = "error")]
    Error { message: String },
}

#[derive(Deserialize)]
struct PageImage {
    page: usize,
    mime: String,
    data: String,
}

enum Input {
    Image(String),
    Pdf,
    Unsupported,
}

#[async_trait(?Send)]
impl Tool for OcrTool {
    fn name(&self) -> &str {
        "ocr"
    }

    fn readable_name(&self) -> &str {
        "OCR document"
    }

    fn definition(&self) -> LlmTool {
        LlmTool {
            r#type: llm::ToolType::Function,
            function: Function {
                name: self.name().to_owned(),
                description: "Extract text from a scanned or digital document (PDF or image) as \
                              clean Markdown. Digital PDFs are read directly; scans and images are \
                              transcribed with a vision model. Writes the result to the workspace \
                              and returns its path with a preview."
                    .to_string(),
                parameters: Some(OcrParams::function_parameters()),
            },
        }
    }

    async fn execute(&self, config: Arc<AgentConfig>, args: JsonValue) -> JsonValue {
        let params = match OcrParams::decode(args) {
            Ok(p) => p,
            Err(e) => return json!({ "error": e }),
        };

        let (bytes, mime) = match self.fs.read_bytes(&params.path).await {
            Ok(data) => data,
            Err(e) => return json!({ "error": e.to_string() }),
        };

        match classify(&params.path, mime.as_deref()) {
            Input::Image(mime) => self.ocr_image(&config, &params, &bytes, &mime).await,
            Input::Pdf => self.ocr_pdf(&config, &params).await,
            Input::Unsupported => json!({
                "error": format!("{} is not a supported OCR input (expected an image or PDF)", params.path)
            }),
        }
    }
}

impl OcrTool {
    async fn ocr_image(
        &self,
        config: &AgentConfig,
        params: &OcrParams,
        bytes: &[u8],
        mime: &str,
    ) -> JsonValue {
        let model = match vision_model(config) {
            Ok(model) => model,
            Err(e) => return json!({ "error": e }),
        };
        let image = ImageSource::base64(mime.to_string(), STANDARD.encode(bytes));
        let markdown = match transcribe(&model, vec![image], "this image").await {
            Ok(md) => md,
            Err(e) => return json!({ "error": e }),
        };
        self.finalize(params, &markdown, "vision-model", 1, &[])
            .await
    }

    async fn ocr_pdf(&self, config: &AgentConfig, params: &OcrParams) -> JsonValue {
        let Some(service) = self.python.as_ref() else {
            return json!({
                "error": "PDF OCR requires the Python tool, which is disabled on this deployment"
            });
        };

        let output = match service.execute_python(&pdf_script(&params.path)).await {
            Ok(output) => output,
            Err(e) => return json!({ "error": e.to_string() }),
        };
        let parsed: PdfResult = match serde_json::from_str(output.stdout.trim()) {
            Ok(parsed) => parsed,
            Err(_) => {
                return json!({
                    "error": format!("could not read PDF: {}", first_line(&output.stderr))
                });
            }
        };

        match parsed {
            PdfResult::Error { message } => {
                json!({ "error": format!("could not read PDF: {message}") })
            }
            PdfResult::Text { pages, text } => {
                let markdown = clean_text_pages(&text);
                self.finalize(params, &markdown, "text-layer", pages, &[])
                    .await
            }
            PdfResult::Images {
                pages,
                images,
                empty_pages,
            } => {
                let model = match vision_model(config) {
                    Ok(model) => model,
                    Err(e) => return json!({ "error": e }),
                };
                match self.transcribe_pages(&model, images, pages).await {
                    Ok(markdown) => {
                        self.finalize(params, &markdown, "vision-model", pages, &empty_pages)
                            .await
                    }
                    Err(e) => json!({ "error": e }),
                }
            }
        }
    }

    /// Transcribes page images grouped by page, preserving order.
    async fn transcribe_pages(
        &self,
        model: &ModelRegEntry,
        images: Vec<PageImage>,
        total: usize,
    ) -> Result<String, String> {
        let mut sections: Vec<String> = Vec::new();
        let mut index = 0;
        while index < images.len() {
            let page = images[index].page;
            let mut sources = Vec::new();
            while index < images.len() && images[index].page == page {
                let img = &images[index];
                sources.push(ImageSource::base64(img.mime.clone(), img.data.clone()));
                index += 1;
            }
            let hint = format!("page {} of {}", page + 1, total);
            let text = transcribe(model, sources, &hint).await?;
            if !text.trim().is_empty() {
                sections.push(text.trim().to_string());
            }
        }
        Ok(sections.join("\n\n"))
    }

    async fn finalize(
        &self,
        params: &OcrParams,
        markdown: &str,
        source: &str,
        pages: usize,
        empty_pages: &[usize],
    ) -> JsonValue {
        let output_path = params
            .output_path
            .clone()
            .unwrap_or_else(|| default_output_path(&self.writable_root, &params.path));

        if let Some(parent) = parent_dir(&output_path) {
            let _ = self.fs.create_dir(&parent).await;
        }
        if let Err(e) = self
            .fs
            .write_bytes(&output_path, markdown.as_bytes(), Some("text/markdown"))
            .await
        {
            return json!({ "error": e.to_string() });
        }

        let mut result = json!({
            "success": true,
            "output_path": output_path,
            "source": source,
            "pages": pages,
            "chars": markdown.chars().count(),
            "preview": preview(markdown),
        });
        if pages > MAX_PAGES {
            result["truncated"] = json!(format!("only the first {MAX_PAGES} pages were processed"));
        }
        if !empty_pages.is_empty() {
            let human: Vec<usize> = empty_pages.iter().map(|p| p + 1).collect();
            result["pages_without_content"] = json!(human);
        }
        result
    }
}

/// Resolves the OCR model, requiring vision support for image transcription.
fn vision_model(config: &AgentConfig) -> Result<ModelRegEntry, String> {
    let model = config
        .model_registry
        .get(OCR_MODEL)
        .cloned()
        .unwrap_or_else(|| config.model_registry.get_or_default(DEFAULT_MODEL).clone());
    if !model.vision {
        return Err(
            "no vision-capable model is configured for OCR; add a [models.ocr] entry with \
                    vision = true"
                .to_string(),
        );
    }
    Ok(model)
}

async fn transcribe(
    model: &ModelRegEntry,
    images: Vec<ImageSource>,
    hint: &str,
) -> Result<String, String> {
    let messages = vec![
        Message {
            role: Role::System,
            content: SYSTEM_PROMPT.to_string(),
            ..Default::default()
        },
        Message {
            role: Role::User,
            content: format!("Transcribe {hint} into Markdown."),
            images: Some(images),
            ..Default::default()
        },
    ];
    let mut request = CompletionRequest::new(&model.model_name, &messages).max_tokens(8192);
    if let Some(effort) = model.reasoning_effort {
        request = request.reasoning_effort(effort);
    }
    let completion = model
        .api
        .get_completion(&model.token, request)
        .await
        .map_err(|e| e.to_string())?;
    let text = completion
        .choices
        .first()
        .and_then(|choice| {
            choice
                .message
                .as_ref()
                .map(|message| message.content.clone())
                .or_else(|| choice.text.clone())
        })
        .unwrap_or_default();
    Ok(text)
}

fn classify(path: &str, mime: Option<&str>) -> Input {
    if let Some(mime) = mime {
        if mime.starts_with("image/") {
            return Input::Image(mime.to_string());
        }
        if mime == "application/pdf" {
            return Input::Pdf;
        }
    }
    match extension(path).as_deref() {
        Some("png") => Input::Image("image/png".to_string()),
        Some("jpg" | "jpeg") => Input::Image("image/jpeg".to_string()),
        Some("webp") => Input::Image("image/webp".to_string()),
        Some("gif") => Input::Image("image/gif".to_string()),
        Some("tif" | "tiff") => Input::Image("image/tiff".to_string()),
        Some("pdf") => Input::Pdf,
        _ => Input::Unsupported,
    }
}

/// Collapses runs of blank lines and trims each page, then joins pages.
fn clean_text_pages(pages: &[String]) -> String {
    let cleaned: Vec<String> = pages
        .iter()
        .map(|page| {
            page.lines()
                .map(str::trim_end)
                .collect::<Vec<_>>()
                .join("\n")
                .trim()
                .to_string()
        })
        .filter(|page| !page.is_empty())
        .collect();
    cleaned.join("\n\n")
}

fn default_output_path(writable_root: &str, input: &str) -> String {
    let stem = input
        .rsplit('/')
        .next()
        .and_then(|name| name.rsplit_once('.').map(|(base, _)| base).or(Some(name)))
        .filter(|s| !s.is_empty())
        .unwrap_or("document");
    format!("{}/ocr/{stem}.md", writable_root.trim_end_matches('/'))
}

fn parent_dir(path: &str) -> Option<String> {
    path.rsplit_once('/').map(|(parent, _)| parent.to_string())
}

fn extension(path: &str) -> Option<String> {
    path.rsplit('/')
        .next()
        .and_then(|name| name.rsplit_once('.'))
        .map(|(_, ext)| ext.to_ascii_lowercase())
}

fn preview(markdown: &str) -> String {
    let trimmed = markdown.trim();
    if trimmed.chars().count() <= PREVIEW_CHARS {
        return trimmed.to_string();
    }
    let cut: String = trimmed.chars().take(PREVIEW_CHARS).collect();
    format!("{cut}…")
}

fn first_line(text: &str) -> String {
    let line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    if line.is_empty() {
        "unreadable PDF".to_string()
    } else {
        line.trim().to_string()
    }
}

/// Preprocessing script template. It first tries the text layer (the free,
/// exact fast path) and only falls back to extracting page images when the PDF
/// has no usable text. `PATH` and `MAX_PAGES` are substituted per call.
const PDF_SCRIPT: &str = include_str!("ocr/pdf_extract.py");

fn pdf_script(path: &str) -> String {
    let path_literal = serde_json::to_string(path).unwrap_or_else(|_| "\"\"".to_string());
    PDF_SCRIPT
        .replace("\"__OCR_PATH__\"", &path_literal)
        .replace("__OCR_MAX_PAGES__", &MAX_PAGES.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_uses_mime_then_extension() {
        assert!(matches!(
            classify("/a/b.bin", Some("image/png")),
            Input::Image(_)
        ));
        assert!(matches!(classify("/a/scan.PDF", None), Input::Pdf));
        assert!(matches!(classify("/a/photo.JPEG", None), Input::Image(_)));
        assert!(matches!(classify("/a/notes.txt", None), Input::Unsupported));
    }

    #[test]
    fn default_output_strips_extension_under_writable_root() {
        assert_eq!(
            default_output_path("/~workspace", "/Invoices/bill.pdf"),
            "/~workspace/ocr/bill.md"
        );
        assert_eq!(
            default_output_path("/Projects/Acme/", "/x/scan.tiff"),
            "/Projects/Acme/ocr/scan.md"
        );
    }

    #[test]
    fn clean_text_pages_drops_blank_pages_and_trailing_space() {
        let out = clean_text_pages(&[
            "  Title  \n\n  body  ".to_string(),
            "   ".to_string(),
            "second".to_string(),
        ]);
        assert_eq!(out, "Title\n\n  body\n\nsecond");
    }

    #[test]
    fn pdf_script_injects_a_valid_python_string_literal() {
        let script = pdf_script("/~workspace/a\"b.pdf");
        assert!(script.contains(r#"PATH = "/~workspace/a\"b.pdf""#));
        assert!(script.contains(&format!("MAX_PAGES = {MAX_PAGES}")));
        assert!(!script.contains("__OCR_PATH__"));
        assert!(!script.contains("__OCR_MAX_PAGES__"));
    }
}
