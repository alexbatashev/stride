use std::io::Cursor;

use anyhow::{Context, Result, bail};
use futures::channel::oneshot;
use office_oxide::{Document, DocumentFormat};
use pdf_oxide::{PdfDocument, converters::ConversionOptions, writer::DocumentBuilder};

const ASYNC_THRESHOLD: usize = 1024 * 1024;

pub async fn convert_to_markdown(data: Vec<u8>) -> Result<String> {
    run_conversion(data.len(), move || convert_to_markdown_sync(data)).await
}

pub async fn markdown_to_pdf(data: String) -> Result<Vec<u8>> {
    run_conversion(data.len(), move || markdown_to_pdf_sync(&data)).await
}

pub async fn markdown_to_office_word(data: String) -> Result<Vec<u8>> {
    run_conversion(data.len(), move || markdown_to_office_word_sync(&data)).await
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

fn markdown_to_pdf_sync(markdown: &str) -> Result<Vec<u8>> {
    let mut builder = DocumentBuilder::new();
    {
        let mut page = builder.letter_page().at(72.0, 720.0);
        for line in markdown.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                page = page.space(8.0);
                continue;
            }

            if let Some((level, text)) = markdown_heading(trimmed) {
                page = page.heading(level, text).space(6.0);
            } else if let Some(item) = trimmed.strip_prefix("- ") {
                page = page.bullet_list(&[item]);
            } else {
                page = page.text(trimmed);
            }
        }
        page.done();
    }
    builder.build().context("write PDF")
}

fn markdown_heading(line: &str) -> Option<(u8, &str)> {
    let level = line.chars().take_while(|&ch| ch == '#').count();
    if level == 0 || level > 6 {
        return None;
    }
    let text = line[level..].trim();
    (!text.is_empty()).then_some((level as u8, text))
}

fn markdown_to_office_word_sync(markdown: &str) -> Result<Vec<u8>> {
    let mut writer = Cursor::new(Vec::new());
    office_oxide::create::create_from_markdown_to_writer(
        markdown,
        DocumentFormat::Docx,
        &mut writer,
    )
    .context("write DOCX")?;
    Ok(writer.into_inner())
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
    fn markdown_to_pdf_writes_pdf_bytes() {
        let bytes = block_on(markdown_to_pdf("# Report\n\nHello.".to_string())).unwrap();

        assert!(bytes.starts_with(b"%PDF-"));
    }
}
