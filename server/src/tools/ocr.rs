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

/// Structured page prompt used for scanned PDFs: the model returns Markdown plus
/// the bounding boxes of figures/diagrams so the tool can crop them out of the
/// page raster and embed them in the reconstruction.
const PAGE_SYSTEM_PROMPT: &str = "You are an OCR engine. Return ONLY a single JSON object, no \
prose and no code fence, with this shape: {\"markdown\": string, \"figures\": [{\"caption\": \
string, \"box\": [x0, y0, x1, y1]}]}. In \"markdown\", transcribe the page verbatim into clean \
GitHub-flavored Markdown: headings with #, lists, and tables using Markdown table syntax, in the \
original reading order and language; do not translate, summarize, or add commentary. For every \
figure, diagram, chart, or illustration (anything that is not plain text or a table), do NOT \
transcribe its contents: instead place a placeholder \"[[FIGURE:k]]\" on its own line where it \
belongs (k = 1 for the first figure on the page, 2 for the second, and so on), and add an entry \
to \"figures\" whose \"box\" is the figure's bounding box in a 0-1000 coordinate space relative to \
the image (x0,y0 = top-left, x1,y1 = bottom-right) and whose \"caption\" is the figure's caption \
text if present (else a short description). If the page has no figures, use an empty \"figures\" \
list.";

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
    /// Born-digital PDF: layout-aware Markdown assembled from the text layer
    /// (headings, paragraphs, tables) by the preprocessing script.
    #[serde(rename = "text")]
    Text { pages: usize, markdown: String },
    /// Scanned PDF: one full-page raster per page, transcribed with the vision
    /// model.
    #[serde(rename = "page_images")]
    PageImages {
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

/// One page transcription: Markdown (with `[[FIGURE:k]]` placeholders) plus the
/// figure regions the vision model located on the page.
struct PageTranscript {
    markdown: String,
    figures: Vec<VisionFigure>,
}

#[derive(Deserialize, Default)]
struct VisionResponse {
    #[serde(default)]
    markdown: String,
    #[serde(default)]
    figures: Vec<VisionFigure>,
}

#[derive(Deserialize)]
struct VisionFigure {
    #[serde(default)]
    caption: String,
    /// `[x0, y0, x1, y1]` in a 0-1000 space relative to the page image.
    #[serde(rename = "box")]
    r#box: [f64; 4],
}

#[derive(Deserialize)]
struct CropResult {
    crops: Vec<Crop>,
}

#[derive(Deserialize)]
struct Crop {
    n: usize,
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
            PdfResult::Text { pages, markdown } => {
                self.finalize(params, &markdown, "text-layer", pages, &[])
                    .await
            }
            PdfResult::PageImages {
                pages,
                images,
                empty_pages,
            } => {
                let model = match vision_model(config) {
                    Ok(model) => model,
                    Err(e) => return json!({ "error": e }),
                };
                let output_path = self.resolve_output_path(params);
                match self
                    .transcribe_pages_inner(&model, &params.path, &output_path, images, pages)
                    .await
                {
                    Ok((markdown, assets)) => {
                        self.finalize_with_assets(
                            params,
                            &markdown,
                            "vision-model",
                            pages,
                            &empty_pages,
                            &assets,
                        )
                        .await
                    }
                    Err(e) => json!({ "error": e }),
                }
            }
        }
    }

    /// Transcribes page rasters to Markdown and crops each figure the model
    /// reports out of its page. Returns the combined Markdown (with
    /// `![caption](assets/figN.png)` references) and the figure assets to write.
    async fn transcribe_pages_inner(
        &self,
        model: &ModelRegEntry,
        pdf_path: &str,
        output_path: &str,
        images: Vec<PageImage>,
        total: usize,
    ) -> Result<(String, Vec<(String, Vec<u8>)>), String> {
        let assets_rel = assets_dir_name(output_path);

        let mut sections: Vec<String> = Vec::new();
        let mut crop_requests: Vec<JsonValue> = Vec::new();
        let mut fig_counter = 0usize;
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
            let transcript = transcribe_page(model, sources, &hint).await?;
            let mut md = transcript.markdown.trim().to_string();
            for (local, fig) in transcript.figures.iter().enumerate() {
                fig_counter += 1;
                let asset = format!("{assets_rel}/fig{fig_counter}.png");
                crop_requests.push(json!({ "n": fig_counter, "page": page, "box": fig.r#box }));
                let image_md = format!("![{}]({asset})", fig.caption.replace(['[', ']'], ""));
                let placeholder = format!("[[FIGURE:{}]]", local + 1);
                if md.contains(&placeholder) {
                    md = md.replace(&placeholder, &image_md);
                } else {
                    md.push_str("\n\n");
                    md.push_str(&image_md);
                }
            }
            if !md.is_empty() {
                sections.push(md);
            }
        }

        let markdown = sections.join("\n\n");
        let assets = if crop_requests.is_empty() {
            Vec::new()
        } else {
            self.crop_figures(pdf_path, &crop_requests, &assets_rel)
                .await
        };
        Ok((markdown, assets))
    }

    /// Runs the crop script in the Python sandbox and returns `(relative-path,
    /// png-bytes)` for each figure it managed to cut out. Failures drop the
    /// figure's image (the caption text still remains in the Markdown).
    async fn crop_figures(
        &self,
        pdf_path: &str,
        requests: &[JsonValue],
        assets_rel: &str,
    ) -> Vec<(String, Vec<u8>)> {
        let Some(service) = self.python.as_ref() else {
            return Vec::new();
        };
        let script = crop_script(pdf_path, requests);
        let output = match service.execute_python(&script).await {
            Ok(output) => output,
            Err(_) => return Vec::new(),
        };
        let parsed: CropResult = match serde_json::from_str(output.stdout.trim()) {
            Ok(parsed) => parsed,
            Err(_) => return Vec::new(),
        };
        parsed
            .crops
            .into_iter()
            .filter_map(|crop| {
                let bytes = STANDARD.decode(crop.data.as_bytes()).ok()?;
                Some((format!("{assets_rel}/fig{}.png", crop.n), bytes))
            })
            .collect()
    }

    fn resolve_output_path(&self, params: &OcrParams) -> String {
        params
            .output_path
            .clone()
            .unwrap_or_else(|| default_output_path(&self.writable_root, &params.path))
    }

    async fn finalize(
        &self,
        params: &OcrParams,
        markdown: &str,
        source: &str,
        pages: usize,
        empty_pages: &[usize],
    ) -> JsonValue {
        self.finalize_with_assets(params, markdown, source, pages, empty_pages, &[])
            .await
    }

    /// Writes the Markdown and any figure assets (paths relative to the Markdown
    /// file) to the file system, then returns the tool result.
    async fn finalize_with_assets(
        &self,
        params: &OcrParams,
        markdown: &str,
        source: &str,
        pages: usize,
        empty_pages: &[usize],
        assets: &[(String, Vec<u8>)],
    ) -> JsonValue {
        let output_path = self.resolve_output_path(params);
        let base_dir = parent_dir(&output_path);

        if let Some(parent) = &base_dir {
            let _ = self.fs.create_dir(parent).await;
        }
        for (rel, bytes) in assets {
            let abs = match &base_dir {
                Some(dir) => format!("{}/{rel}", dir.trim_end_matches('/')),
                None => rel.clone(),
            };
            if let Some(parent) = parent_dir(&abs) {
                let _ = self.fs.create_dir(&parent).await;
            }
            let _ = self.fs.write_bytes(&abs, bytes, Some("image/png")).await;
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
        if !assets.is_empty() {
            result["figures"] = json!(assets.len());
        }
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

async fn vision_completion(
    model: &ModelRegEntry,
    system: &str,
    user: String,
    images: Vec<ImageSource>,
) -> Result<String, String> {
    let messages = vec![
        Message {
            role: Role::System,
            content: system.to_string(),
            ..Default::default()
        },
        Message {
            role: Role::User,
            content: user,
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
    Ok(completion
        .choices
        .first()
        .and_then(|choice| {
            choice
                .message
                .as_ref()
                .map(|message| message.content.clone())
                .or_else(|| choice.text.clone())
        })
        .unwrap_or_default())
}

async fn transcribe(
    model: &ModelRegEntry,
    images: Vec<ImageSource>,
    hint: &str,
) -> Result<String, String> {
    vision_completion(
        model,
        SYSTEM_PROMPT,
        format!("Transcribe {hint} into Markdown."),
        images,
    )
    .await
}

/// Transcribes a scanned page, returning Markdown plus located figure boxes.
/// Falls back to treating the whole reply as Markdown if the model does not
/// return the requested JSON.
async fn transcribe_page(
    model: &ModelRegEntry,
    images: Vec<ImageSource>,
    hint: &str,
) -> Result<PageTranscript, String> {
    let raw = vision_completion(
        model,
        PAGE_SYSTEM_PROMPT,
        format!("Transcribe {hint}. Return only the JSON object."),
        images,
    )
    .await?;
    match extract_json(&raw).and_then(|j| serde_json::from_str::<VisionResponse>(j).ok()) {
        Some(response) => Ok(PageTranscript {
            markdown: response.markdown,
            figures: response.figures,
        }),
        None => Ok(PageTranscript {
            markdown: raw,
            figures: Vec::new(),
        }),
    }
}

/// Extracts the outermost JSON object from a model reply, tolerating a stray
/// code fence or surrounding prose.
fn extract_json(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    if end > start {
        Some(&text[start..=end])
    } else {
        None
    }
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

/// Preprocessing script template. Born-digital PDFs are assembled into
/// layout-aware Markdown from the text layer; scanned PDFs return page rasters
/// for the vision model. `PATH` and `MAX_PAGES` are substituted per call.
const PDF_SCRIPT: &str = include_str!("ocr/pdf_extract.py");

fn pdf_script(path: &str) -> String {
    let path_literal = serde_json::to_string(path).unwrap_or_else(|_| "\"\"".to_string());
    PDF_SCRIPT
        .replace("\"__OCR_PATH__\"", &path_literal)
        .replace("__OCR_MAX_PAGES__", &MAX_PAGES.to_string())
}

/// Directory name (relative to the Markdown file) that holds cropped figures,
/// derived from the output file stem: `king.md` -> `king_assets`.
fn assets_dir_name(output_path: &str) -> String {
    let stem = output_path
        .rsplit('/')
        .next()
        .and_then(|name| name.rsplit_once('.').map(|(base, _)| base).or(Some(name)))
        .filter(|s| !s.is_empty())
        .unwrap_or("document");
    format!("{stem}_assets")
}

/// Figure-cropping script template. `PATH` and the JSON crop-request list are
/// substituted per call.
const CROP_SCRIPT: &str = include_str!("ocr/crop_figures.py");

fn crop_script(path: &str, requests: &[JsonValue]) -> String {
    let path_literal = serde_json::to_string(path).unwrap_or_else(|_| "\"\"".to_string());
    let requests_literal = serde_json::to_string(requests).unwrap_or_else(|_| "[]".to_string());
    CROP_SCRIPT
        .replace("\"__OCR_PATH__\"", &path_literal)
        .replace("__CROP_REQUESTS__", &requests_literal)
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
    fn pdf_script_injects_a_valid_python_string_literal() {
        let script = pdf_script("/~workspace/a\"b.pdf");
        assert!(script.contains(r#"PATH = "/~workspace/a\"b.pdf""#));
        assert!(script.contains(&format!("MAX_PAGES = {MAX_PAGES}")));
        assert!(!script.contains("__OCR_PATH__"));
        assert!(!script.contains("__OCR_MAX_PAGES__"));
    }
}
