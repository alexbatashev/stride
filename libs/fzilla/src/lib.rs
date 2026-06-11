//! Document conversion for LLM tools: PDF/Office documents to Markdown, and
//! Markdown / presentation XML to PDF, DOCX, and PPTX.

mod docx;
mod markdown;
mod pdf;
mod pptx;

use std::io::Cursor;

use anyhow::{Context, Result, bail};
use futures::channel::oneshot;
use office_oxide::{Document, DocumentFormat};
use pdf_oxide::{PdfDocument, converters::ConversionOptions};

pub use pptx::PresentationImageAsset;

const ASYNC_THRESHOLD: usize = 1024 * 1024;

pub async fn convert_to_markdown(data: Vec<u8>) -> Result<String> {
    run_conversion(data.len(), move || convert_to_markdown_sync(data)).await
}

pub async fn markdown_to_pdf(data: String) -> Result<Vec<u8>> {
    run_conversion(data.len(), move || pdf::render(&data)).await
}

pub async fn markdown_to_office_word(data: String) -> Result<Vec<u8>> {
    run_conversion(data.len(), move || docx::render(&data)).await
}

pub async fn presentation_xml_to_pptx(data: String) -> Result<Vec<u8>> {
    run_conversion(data.len(), move || pptx::render(&data, Vec::new())).await
}

pub async fn presentation_xml_to_pptx_with_assets(
    data: String,
    assets: Vec<PresentationImageAsset>,
) -> Result<Vec<u8>> {
    let size = data.len() + assets.iter().map(|asset| asset.data.len()).sum::<usize>();
    run_conversion(size, move || pptx::render(&data, assets)).await
}

pub fn presentation_xml_image_paths(xml: &str) -> Result<Vec<String>> {
    pptx::image_paths(xml)
}

async fn run_conversion<F, T>(size: usize, f: F) -> Result<T>
where
    F: FnOnce() -> Result<T> + Send + 'static,
    T: Send + 'static,
{
    if size < ASYNC_THRESHOLD {
        return f();
    }

    let (tx, rx) = oneshot::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    rx.await.context("conversion thread stopped")?
}

fn convert_to_markdown_sync(data: Vec<u8>) -> Result<String> {
    if data.starts_with(b"%PDF-") {
        let doc = PdfDocument::from_bytes(data).context("parse PDF")?;
        return doc
            .to_markdown_all(&ConversionOptions::default())
            .context("convert PDF to Markdown");
    }

    let format = detect_office_format(&data)?;
    let doc = Document::from_reader(Cursor::new(data), format).context("parse Office document")?;
    Ok(doc.to_markdown())
}

fn detect_office_format(data: &[u8]) -> Result<DocumentFormat> {
    if data.starts_with(b"PK") {
        if contains_ascii(data, b"word/document.xml") {
            return Ok(DocumentFormat::Docx);
        }
        if contains_ascii(data, b"xl/workbook.xml") {
            return Ok(DocumentFormat::Xlsx);
        }
        if contains_ascii(data, b"ppt/presentation.xml") {
            return Ok(DocumentFormat::Pptx);
        }
    }

    if data.starts_with(&[0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]) {
        if contains_utf16le(data, "WordDocument") {
            return Ok(DocumentFormat::Doc);
        }
        if contains_utf16le(data, "Workbook") || contains_utf16le(data, "Book") {
            return Ok(DocumentFormat::Xls);
        }
        if contains_utf16le(data, "PowerPoint Document") {
            return Ok(DocumentFormat::Ppt);
        }
    }

    bail!("unsupported document format")
}

fn contains_ascii(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

fn contains_utf16le(haystack: &[u8], needle: &str) -> bool {
    let needle: Vec<u8> = needle
        .encode_utf16()
        .flat_map(|ch| ch.to_le_bytes())
        .collect();
    contains_ascii(haystack, &needle)
}

#[cfg(test)]
mod tests {
    use futures::executor::block_on;

    use super::*;

    #[test]
    fn markdown_to_docx_round_trips_to_markdown() {
        let bytes = block_on(markdown_to_office_word(
            "# Report\n\nHello **world**.".to_string(),
        ))
        .unwrap();

        let markdown = block_on(convert_to_markdown(bytes)).unwrap();

        assert!(markdown.contains("Report"));
        assert!(markdown.contains("Hello"));
    }

    #[test]
    fn markdown_to_docx_uses_comrak_markup() {
        let bytes = block_on(markdown_to_office_word(
            "Visit [Frontiers](https://example.com) with **bold** and *italic*.\n\n| A | B |\n| - | - |\n| 1 | 2 |".to_string(),
        ))
        .unwrap();

        let markdown = block_on(convert_to_markdown(bytes)).unwrap();

        assert!(markdown.contains("Frontiers"));
        assert!(markdown.contains("**bold**"));
        assert!(markdown.contains("*italic*"));
        assert!(markdown.contains("| A | B |"));
    }

    #[test]
    fn markdown_to_pdf_writes_pdf_bytes() {
        let bytes = block_on(markdown_to_pdf("# Report\n\nHello.".to_string())).unwrap();

        assert!(bytes.starts_with(b"%PDF-"));
    }

    #[test]
    fn markdown_to_pdf_preserves_links_and_renders_markup() {
        let bytes = block_on(markdown_to_pdf(
            "Visit [Frontiers](https://example.com) with **bold** and *italic*.".to_string(),
        ))
        .unwrap();

        assert!(contains_ascii(&bytes, b"https://example.com"));
        assert!(!contains_ascii(&bytes, b"**bold**"));
        assert!(!contains_ascii(&bytes, b"*italic*"));

        let markdown = block_on(convert_to_markdown(bytes)).unwrap();
        assert!(markdown.contains("Frontiers"));
        assert!(markdown.contains("bold"));
        assert!(markdown.contains("italic"));
    }

    #[test]
    fn markdown_to_pdf_numbers_ordered_lists() {
        let bytes = block_on(markdown_to_pdf(
            "5. five\n6. six\n7. seven\n\nAnd:\n\n1. alpha\n2. beta\n   - nested\n3. gamma"
                .to_string(),
        ))
        .unwrap();

        for marker in ["5.", "6.", "7.", "1.", "2.", "3."] {
            assert!(
                contains_ascii(&bytes, marker.as_bytes()),
                "missing marker {marker}"
            );
        }

        let markdown = block_on(convert_to_markdown(bytes)).unwrap();
        assert!(markdown.contains("seven"));
        assert!(markdown.contains("nested"));
    }

    #[test]
    fn markdown_to_pdf_handles_long_unbreakable_content() {
        let url = format!("https://example.com/{}", "a".repeat(400));
        let code = "x".repeat(600);
        let bytes = block_on(markdown_to_pdf(format!(
            "See [docs]({url}) and {url}\n\n```\n{code}\n```\n\n| A | B |\n| - | - |\n| {code} | 2 |"
        )))
        .unwrap();

        assert!(bytes.starts_with(b"%PDF-"));
        let markdown = block_on(convert_to_markdown(bytes)).unwrap();
        assert!(markdown.contains("docs"));
    }

    #[test]
    fn markdown_to_pdf_renders_tables_as_cells() {
        let bytes = block_on(markdown_to_pdf(
            "| Paper | Year | Technique | Target | Key Result |\n| - | - | - | - | - |\n| RL4ReAl | 2023 | Multi-agent RL | LLVM x86 | Matches allocators |\n| VeriLocc | 2025 | LLM + formal verification | AMD GPUs | >10% over rocBLAS |"
                .to_string(),
        ))
        .unwrap();

        assert!(!contains_ascii(&bytes, b"Paper | Year"));
        assert!(!contains_ascii(&bytes, b"RL4ReAl | 2023"));

        let markdown = block_on(convert_to_markdown(bytes)).unwrap();
        assert!(markdown.contains("Paper"));
        assert!(markdown.contains("VeriLocc"));
    }

    #[test]
    fn markdown_to_pdf_paginates_long_documents() {
        let markdown = (0..180)
            .map(|idx| {
                format!(
                    "Paragraph {idx}: this line is intentionally long enough to wrap across the page and force the renderer to keep laying out text after the first page."
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        let bytes = block_on(markdown_to_pdf(markdown)).unwrap();
        let page_markers = bytes
            .windows(b"/Type /Page".len())
            .filter(|window| *window == b"/Type /Page")
            .count();

        assert!(page_markers > 2, "page markers: {page_markers}");
    }

    #[test]
    fn presentation_xml_to_pptx_round_trips_to_markdown() {
        let bytes = block_on(presentation_xml_to_pptx_with_assets(
            r#"
            <presentation size="wide">
              <slide layout="title">
                <title>Roadmap</title>
                <subtitle>Conference draft</subtitle>
                <notes>Open with project motivation.</notes>
              </slide>
              <slide layout="two-column">
                <title>Quarterly direction</title>
                <columns>
                  <left>
                    <bullets>
                      <item>Ship VFS documents</item>
                      <item>Create presentations</item>
                    </bullets>
                  </left>
                  <right>
                    <image src="/figures/chart.png" w="3" h="2"/>
                  </right>
                </columns>
                <textbox x="0.5" y="6.8" w="12" h="0.3">Confidential</textbox>
              </slide>
            </presentation>
            "#
            .to_string(),
            vec![PresentationImageAsset {
                path: "/figures/chart.png".to_string(),
                data: vec![137, 80, 78, 71, 13, 10, 26, 10],
                mime_type: Some("image/png".to_string()),
            }],
        ))
        .unwrap();

        let markdown = block_on(convert_to_markdown(bytes)).unwrap();

        assert!(markdown.contains("Roadmap"));
        assert!(markdown.contains("Conference draft"));
        assert!(markdown.contains("Quarterly direction"));
        assert!(markdown.contains("Ship VFS documents"));
        assert!(markdown.contains("Confidential"));
        assert!(markdown.contains("Open with project motivation"));
    }
}
