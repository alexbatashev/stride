//! Presentation XML to PPTX rendering, including speaker notes injection
//! (office_oxide has no notes API, so the generated archive is patched).

use std::{
    collections::{HashMap, HashSet},
    io::{Cursor, Read, Seek, Write},
};

use anyhow::{Context, Result, bail};
use office_oxide::{ir as office_ir, pptx::write as pptx_write};
use quick_xml::{
    Decoder, XmlVersion,
    events::{BytesStart, Event},
    name::QName,
    reader::Reader,
};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

#[derive(Clone)]
pub struct PresentationImageAsset {
    pub path: String,
    pub data: Vec<u8>,
    pub mime_type: Option<String>,
}

pub fn image_paths(xml: &str) -> Result<Vec<String>> {
    let presentation = parse_presentation_xml(xml)?;
    let mut seen = HashSet::new();
    let mut paths = Vec::new();
    for slide in &presentation.slides {
        collect_image_paths(&slide.items, &mut seen, &mut paths);
    }
    Ok(paths)
}

pub fn render(xml: &str, assets: Vec<PresentationImageAsset>) -> Result<Vec<u8>> {
    let presentation = parse_presentation_xml(xml)?;
    let slide_count = presentation.slides.len();
    let notes: Vec<Option<String>> = presentation
        .slides
        .iter()
        .map(|slide| slide.notes.clone())
        .collect();
    let size = presentation.size();
    let assets = image_asset_map(assets)?;
    let mut writer = pptx_write::PptxWriter::new();
    match presentation.size.as_deref() {
        Some("standard") => {
            writer.set_presentation_size(9_144_000, 6_858_000);
        }
        Some("wide") | None => {}
        Some(size) => bail!("unsupported presentation size '{size}'"),
    }

    for slide in presentation.slides {
        let out = writer.add_slide();
        render_slide(out, slide, size, &assets)?;
    }

    let mut bytes = Cursor::new(Vec::new());
    writer.write_to(&mut bytes).context("write PPTX")?;
    let bytes = bytes.into_inner();
    if notes.iter().any(Option::is_some) {
        add_notes_to_pptx(bytes, &notes, slide_count)
    } else {
        Ok(bytes)
    }
}

fn render_slide(
    out: &mut pptx_write::SlideData,
    slide: PresentationSlide,
    size: PresentationSize,
    assets: &HashMap<String, (Vec<u8>, office_ir::ImageFormat)>,
) -> Result<()> {
    match slide.layout {
        SlideLayout::Title => render_title_slide(out, &slide, size),
        SlideLayout::Section => render_section_slide(out, &slide, size),
        SlideLayout::TwoColumn => {
            render_content_title(out, slide.title.as_deref(), size);
            for item in slide.items {
                render_body_item(out, item, assets)?;
            }
            Ok(())
        }
        SlideLayout::TitleContent | SlideLayout::Blank => {
            render_content_title(out, slide.title.as_deref(), size);
            let mut y = 1.45;
            render_subtitle(out, slide.subtitle.as_deref(), size, &mut y);
            for item in slide.items {
                render_flow_item(out, item, 0.75, &mut y, size, assets)?;
            }
            Ok(())
        }
    }
}

fn render_content_title(
    out: &mut pptx_write::SlideData,
    title: Option<&str>,
    size: PresentationSize,
) {
    let Some(title) = title else {
        return;
    };
    let (w, _) = size.inches();
    out.add_rich_text_box(
        &[pptx_write::Run::new(title).bold().font_size(28.0)],
        inches_to_emu(0.65),
        inches_to_emu(0.35),
        inches_to_emu(w - 1.3),
        inches_to_emu(0.7),
    );
}

fn render_subtitle(
    out: &mut pptx_write::SlideData,
    subtitle: Option<&str>,
    size: PresentationSize,
    y: &mut f64,
) {
    let Some(subtitle) = subtitle else {
        return;
    };
    let (w, _) = size.inches();
    out.add_rich_text_box(
        &[pptx_write::Run::new(subtitle).font_size(18.0)],
        inches_to_emu(0.75),
        inches_to_emu(*y),
        inches_to_emu(w - 1.5),
        inches_to_emu(0.55),
    );
    *y += 0.7;
}

fn render_title_slide(
    out: &mut pptx_write::SlideData,
    slide: &PresentationSlide,
    size: PresentationSize,
) -> Result<()> {
    let (w, _) = size.inches();
    if let Some(title) = &slide.title {
        out.add_rich_text_box(
            &[pptx_write::Run::new(title).bold().font_size(36.0)],
            inches_to_emu(0.8),
            inches_to_emu(2.1),
            inches_to_emu(w - 1.6),
            inches_to_emu(1.0),
        );
    }
    if let Some(subtitle) = &slide.subtitle {
        out.add_rich_text_box(
            &[pptx_write::Run::new(subtitle).font_size(20.0)],
            inches_to_emu(0.9),
            inches_to_emu(3.2),
            inches_to_emu(w - 1.8),
            inches_to_emu(1.2),
        );
    }
    Ok(())
}

fn render_section_slide(
    out: &mut pptx_write::SlideData,
    slide: &PresentationSlide,
    size: PresentationSize,
) -> Result<()> {
    let (w, _) = size.inches();
    if let Some(title) = &slide.title {
        out.add_rich_text_box(
            &[pptx_write::Run::new(title).bold().font_size(32.0)],
            inches_to_emu(0.9),
            inches_to_emu(2.6),
            inches_to_emu(w - 1.8),
            inches_to_emu(1.0),
        );
    }
    if let Some(subtitle) = &slide.subtitle {
        out.add_text_box(
            subtitle,
            inches_to_emu(0.95),
            inches_to_emu(3.65),
            inches_to_emu(w - 1.9),
            inches_to_emu(0.8),
        );
    }
    Ok(())
}

fn render_flow_item(
    out: &mut pptx_write::SlideData,
    item: PresentationItem,
    x: f64,
    y: &mut f64,
    size: PresentationSize,
    assets: &HashMap<String, (Vec<u8>, office_ir::ImageFormat)>,
) -> Result<()> {
    let (w, _) = size.inches();
    let width = w - x - 0.75;
    match item {
        PresentationItem::Text(text) => {
            let height = text_box_height(&text, 0);
            out.add_rich_text_box(
                &[pptx_write::Run::new(text).font_size(19.0)],
                inches_to_emu(x),
                inches_to_emu(*y),
                inches_to_emu(width),
                inches_to_emu(height),
            );
            *y += height + 0.18;
        }
        PresentationItem::Bullets(items) => {
            let text = bullet_text(&items);
            let height = text_box_height(&text, items.len());
            out.add_rich_text_box(
                &[pptx_write::Run::new(text).font_size(20.0)],
                inches_to_emu(x),
                inches_to_emu(*y),
                inches_to_emu(width),
                inches_to_emu(height),
            );
            *y += height + 0.22;
        }
        PresentationItem::Image {
            src,
            x: image_x,
            y: image_y,
            width,
            height,
        } => {
            let image_y = image_y.unwrap_or(*y);
            let image_height = height.unwrap_or(3.0);
            add_image(
                out,
                assets,
                &src,
                image_x.or(Some(x)),
                Some(image_y),
                width.or(Some(5.5)),
                Some(image_height),
            )?;
            *y = image_y + image_height + 0.22;
        }
        other => render_body_item(out, other, assets)?,
    }
    Ok(())
}

fn render_body_item(
    out: &mut pptx_write::SlideData,
    item: PresentationItem,
    assets: &HashMap<String, (Vec<u8>, office_ir::ImageFormat)>,
) -> Result<()> {
    match item {
        PresentationItem::Text(text) => {
            out.add_rich_text_box(
                &[pptx_write::Run::new(text).font_size(19.0)],
                inches_to_emu(0.75),
                inches_to_emu(1.45),
                inches_to_emu(11.8),
                inches_to_emu(0.7),
            );
        }
        PresentationItem::Bullets(items) => {
            let text = bullet_text(&items);
            out.add_rich_text_box(
                &[pptx_write::Run::new(&text).font_size(20.0)],
                inches_to_emu(0.75),
                inches_to_emu(1.45),
                inches_to_emu(11.8),
                inches_to_emu(text_box_height(&text, items.len())),
            );
        }
        PresentationItem::TextBox {
            text,
            x,
            y,
            width,
            height,
        } => {
            out.add_text_box(
                &text,
                inches_to_emu(x),
                inches_to_emu(y),
                inches_to_emu(width),
                inches_to_emu(height),
            );
        }
        PresentationItem::Image {
            src,
            x,
            y,
            width,
            height,
        } => add_image(out, assets, &src, x, y, width, height)?,
        PresentationItem::Columns { left, right } => {
            render_column(out, left, ColumnBounds::left_wide(), assets)?;
            render_column(out, right, ColumnBounds::right_wide(), assets)?;
        }
    }
    Ok(())
}

fn render_column(
    out: &mut pptx_write::SlideData,
    items: Vec<PresentationItem>,
    bounds: ColumnBounds,
    assets: &HashMap<String, (Vec<u8>, office_ir::ImageFormat)>,
) -> Result<()> {
    let mut y = bounds.y;
    for item in items {
        match item {
            PresentationItem::Text(text) => {
                out.add_rich_text_box(
                    &[pptx_write::Run::new(text).font_size(17.0)],
                    inches_to_emu(bounds.x),
                    inches_to_emu(y),
                    inches_to_emu(bounds.w),
                    inches_to_emu(0.65),
                );
                y += 0.75;
            }
            PresentationItem::Bullets(items) => {
                let text = bullet_text(&items);
                let height = (items.len().max(1) as f64 * 0.35).max(0.7);
                out.add_rich_text_box(
                    &[pptx_write::Run::new(text).font_size(17.5)],
                    inches_to_emu(bounds.x),
                    inches_to_emu(y),
                    inches_to_emu(bounds.w),
                    inches_to_emu(height),
                );
                y += height + 0.25;
            }
            PresentationItem::Image {
                src,
                x,
                y: image_y,
                width,
                height,
            } => {
                let image_y = image_y.unwrap_or(y);
                let height = height.unwrap_or(2.3);
                add_image(
                    out,
                    assets,
                    &src,
                    x.or(Some(bounds.x)),
                    Some(image_y),
                    width.or(Some(bounds.w)),
                    Some(height),
                )?;
                y = image_y + height + 0.25;
            }
            other => render_body_item(out, other, assets)?,
        }
    }
    Ok(())
}

fn bullet_text(items: &[String]) -> String {
    items
        .iter()
        .map(|item| format!("• {item}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn text_box_height(text: &str, item_count: usize) -> f64 {
    let line_count = text.lines().count().max(item_count).max(1) as f64;
    (line_count * 0.42).clamp(0.55, 4.8)
}

fn add_image(
    out: &mut pptx_write::SlideData,
    assets: &HashMap<String, (Vec<u8>, office_ir::ImageFormat)>,
    src: &str,
    x: Option<f64>,
    y: Option<f64>,
    width: Option<f64>,
    height: Option<f64>,
) -> Result<()> {
    let (data, format) = assets
        .get(src)
        .ok_or_else(|| anyhow::anyhow!("missing image asset {src}"))?;
    out.add_image(
        data.clone(),
        format.clone(),
        inches_to_emu(x.unwrap_or(0.8)),
        inches_to_emu(y.unwrap_or(1.4)),
        inches_to_emu_u64(width.unwrap_or(5.0)),
        inches_to_emu_u64(height.unwrap_or(3.0)),
    );
    Ok(())
}

struct PresentationXml {
    size: Option<String>,
    slides: Vec<PresentationSlide>,
}

impl PresentationXml {
    fn size(&self) -> PresentationSize {
        if self.size.as_deref() == Some("standard") {
            PresentationSize::Standard
        } else {
            PresentationSize::Wide
        }
    }
}

#[derive(Clone, Copy)]
enum PresentationSize {
    Wide,
    Standard,
}

impl PresentationSize {
    fn inches(self) -> (f64, f64) {
        match self {
            Self::Wide => (13.333, 7.5),
            Self::Standard => (10.0, 7.5),
        }
    }
}

#[derive(Default)]
struct PresentationSlide {
    layout: SlideLayout,
    title: Option<String>,
    subtitle: Option<String>,
    notes: Option<String>,
    items: Vec<PresentationItem>,
}

#[derive(Default)]
enum SlideLayout {
    #[default]
    TitleContent,
    Title,
    Section,
    TwoColumn,
    Blank,
}

enum PresentationItem {
    Text(String),
    Bullets(Vec<String>),
    TextBox {
        text: String,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    },
    Image {
        src: String,
        x: Option<f64>,
        y: Option<f64>,
        width: Option<f64>,
        height: Option<f64>,
    },
    Columns {
        left: Vec<PresentationItem>,
        right: Vec<PresentationItem>,
    },
}

#[derive(Clone, Copy)]
struct ColumnBounds {
    x: f64,
    y: f64,
    w: f64,
}

impl ColumnBounds {
    fn left_wide() -> Self {
        Self {
            x: 0.75,
            y: 1.45,
            w: 5.8,
        }
    }

    fn right_wide() -> Self {
        Self {
            x: 6.85,
            y: 1.45,
            w: 5.8,
        }
    }
}

fn parse_presentation_xml(xml: &str) -> Result<PresentationXml> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);

    let mut presentation = PresentationXml {
        size: None,
        slides: Vec::new(),
    };

    loop {
        match reader.read_event().context("parse presentation XML")? {
            Event::Start(e) if e.name().as_ref() == b"presentation" => {
                presentation.size = xml_attr(&e, b"size", reader.decoder())?;
            }
            Event::Empty(e) if e.name().as_ref() == b"presentation" => {
                presentation.size = xml_attr(&e, b"size", reader.decoder())?;
            }
            Event::Start(e) if e.name().as_ref() == b"slide" => {
                presentation
                    .slides
                    .push(parse_slide(&mut reader, &e).context("parse slide")?);
            }
            Event::Eof => break,
            _ => {}
        }
    }

    if presentation.slides.is_empty() {
        bail!("presentation must contain at least one slide");
    }

    Ok(presentation)
}

fn parse_slide(reader: &mut Reader<&[u8]>, start: &BytesStart<'_>) -> Result<PresentationSlide> {
    let mut slide = PresentationSlide {
        layout: parse_slide_layout(xml_attr(start, b"layout", reader.decoder())?.as_deref())?,
        ..Default::default()
    };

    loop {
        match reader.read_event().context("parse slide XML")? {
            Event::Start(e) if e.name().as_ref() == b"title" => {
                slide.title = Some(read_element_text(reader, b"title")?);
            }
            Event::Start(e) if e.name().as_ref() == b"subtitle" => {
                slide.subtitle = Some(read_element_text(reader, b"subtitle")?);
            }
            Event::Start(e) if e.name().as_ref() == b"notes" => {
                slide.notes = Some(read_element_text(reader, b"notes")?);
            }
            Event::Start(e) if e.name().as_ref() == b"columns" => {
                let (left, right) = parse_columns(reader)?;
                slide.items.push(PresentationItem::Columns { left, right });
            }
            Event::Start(e) => {
                if let Some(item) = parse_item(reader, e)? {
                    slide.items.push(item);
                }
            }
            Event::Empty(e) => {
                if let Some(item) = parse_empty_item(reader, e)? {
                    slide.items.push(item);
                }
            }
            Event::End(e) if e.name().as_ref() == b"slide" => break,
            Event::Eof => bail!("unexpected EOF in slide"),
            _ => {}
        }
    }

    Ok(slide)
}

fn parse_columns(
    reader: &mut Reader<&[u8]>,
) -> Result<(Vec<PresentationItem>, Vec<PresentationItem>)> {
    let mut left = Vec::new();
    let mut right = Vec::new();

    loop {
        match reader.read_event().context("parse columns XML")? {
            Event::Start(e) if e.name().as_ref() == b"left" => {
                left = parse_item_container(reader, b"left")?;
            }
            Event::Start(e) if e.name().as_ref() == b"right" => {
                right = parse_item_container(reader, b"right")?;
            }
            Event::End(e) if e.name().as_ref() == b"columns" => break,
            Event::Eof => bail!("unexpected EOF in columns"),
            _ => {}
        }
    }

    Ok((left, right))
}

fn parse_item_container(reader: &mut Reader<&[u8]>, end: &[u8]) -> Result<Vec<PresentationItem>> {
    let mut items = Vec::new();
    loop {
        match reader.read_event().context("parse item container XML")? {
            Event::Start(e) => {
                if let Some(item) = parse_item(reader, e)? {
                    items.push(item);
                }
            }
            Event::Empty(e) => {
                if let Some(item) = parse_empty_item(reader, e)? {
                    items.push(item);
                }
            }
            Event::End(e) if e.name().as_ref() == end => break,
            Event::Eof => bail!("unexpected EOF in item container"),
            _ => {}
        }
    }
    Ok(items)
}

fn parse_item(reader: &mut Reader<&[u8]>, e: BytesStart<'_>) -> Result<Option<PresentationItem>> {
    match e.name().as_ref() {
        b"text" => {
            let text = read_element_text(reader, b"text")?;
            Ok((!text.is_empty()).then_some(PresentationItem::Text(text)))
        }
        b"textbox" => {
            let decoder = reader.decoder();
            let x = required_f64_attr(&e, b"x", decoder)?;
            let y = required_f64_attr(&e, b"y", decoder)?;
            let width = required_f64_attr(&e, b"w", decoder)?;
            let height = required_f64_attr(&e, b"h", decoder)?;
            let text = read_element_text(reader, b"textbox")?;
            Ok(Some(PresentationItem::TextBox {
                text,
                x,
                y,
                width,
                height,
            }))
        }
        b"bullets" => Ok(Some(PresentationItem::Bullets(parse_bullets(reader)?))),
        b"image" => {
            let decoder = reader.decoder();
            let item = PresentationItem::Image {
                src: required_attr(&e, b"src", decoder)?,
                x: optional_f64_attr(&e, b"x", decoder)?,
                y: optional_f64_attr(&e, b"y", decoder)?,
                width: optional_f64_attr(&e, b"w", decoder)?,
                height: optional_f64_attr(&e, b"h", decoder)?,
            };
            reader
                .read_to_end(QName(b"image"))
                .context("parse image end")?;
            Ok(Some(item))
        }
        other => bail!(
            "unsupported presentation XML element {}",
            String::from_utf8_lossy(other)
        ),
    }
}

fn parse_empty_item(
    reader: &mut Reader<&[u8]>,
    e: BytesStart<'_>,
) -> Result<Option<PresentationItem>> {
    match e.name().as_ref() {
        b"image" => {
            let decoder = reader.decoder();
            Ok(Some(PresentationItem::Image {
                src: required_attr(&e, b"src", decoder)?,
                x: optional_f64_attr(&e, b"x", decoder)?,
                y: optional_f64_attr(&e, b"y", decoder)?,
                width: optional_f64_attr(&e, b"w", decoder)?,
                height: optional_f64_attr(&e, b"h", decoder)?,
            }))
        }
        other => bail!(
            "unsupported empty presentation XML element {}",
            String::from_utf8_lossy(other)
        ),
    }
}

fn parse_bullets(reader: &mut Reader<&[u8]>) -> Result<Vec<String>> {
    let mut items = Vec::new();
    loop {
        match reader.read_event().context("parse bullets XML")? {
            Event::Start(e) if e.name().as_ref() == b"item" => {
                items.push(read_element_text(reader, b"item")?);
            }
            Event::End(e) if e.name().as_ref() == b"bullets" => break,
            Event::Eof => bail!("unexpected EOF in bullets"),
            _ => {}
        }
    }
    Ok(items)
}

fn read_element_text(reader: &mut Reader<&[u8]>, end: &[u8]) -> Result<String> {
    let mut text = String::new();
    loop {
        match reader.read_event().context("parse text XML")? {
            Event::Text(e) => text.push_str(&e.decode()?),
            Event::CData(e) => text.push_str(&e.decode()?),
            Event::Start(e) => {
                text.push_str(&read_element_text(reader, e.name().as_ref())?);
            }
            Event::Empty(e) if e.name().as_ref() == b"br" => text.push('\n'),
            Event::End(e) if e.name().as_ref() == end => break,
            Event::Eof => bail!("unexpected EOF in {} text", String::from_utf8_lossy(end)),
            _ => {}
        }
    }
    Ok(text)
}

fn parse_slide_layout(value: Option<&str>) -> Result<SlideLayout> {
    match value.unwrap_or("title-content") {
        "title-content" => Ok(SlideLayout::TitleContent),
        "title" => Ok(SlideLayout::Title),
        "section" => Ok(SlideLayout::Section),
        "two-column" => Ok(SlideLayout::TwoColumn),
        "blank" => Ok(SlideLayout::Blank),
        other => bail!("unsupported slide layout '{other}'"),
    }
}

fn collect_image_paths(
    items: &[PresentationItem],
    seen: &mut HashSet<String>,
    paths: &mut Vec<String>,
) {
    for item in items {
        match item {
            PresentationItem::Image { src, .. } if seen.insert(src.clone()) => {
                paths.push(src.clone());
            }
            PresentationItem::Columns { left, right } => {
                collect_image_paths(left, seen, paths);
                collect_image_paths(right, seen, paths);
            }
            _ => {}
        }
    }
}

fn image_asset_map(
    assets: Vec<PresentationImageAsset>,
) -> Result<HashMap<String, (Vec<u8>, office_ir::ImageFormat)>> {
    let mut map = HashMap::new();
    for asset in assets {
        let format = image_format(&asset.path, asset.mime_type.as_deref())?;
        map.insert(asset.path, (asset.data, format));
    }
    Ok(map)
}

fn image_format(path: &str, mime_type: Option<&str>) -> Result<office_ir::ImageFormat> {
    match mime_type {
        Some("image/png") => return Ok(office_ir::ImageFormat::Png),
        Some("image/jpeg") | Some("image/jpg") => return Ok(office_ir::ImageFormat::Jpeg),
        Some("image/gif") => return Ok(office_ir::ImageFormat::Gif),
        Some("image/tiff") => return Ok(office_ir::ImageFormat::Tiff),
        Some("image/bmp") => return Ok(office_ir::ImageFormat::Bmp),
        _ => {}
    }

    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "png" => Ok(office_ir::ImageFormat::Png),
        "jpg" | "jpeg" => Ok(office_ir::ImageFormat::Jpeg),
        "gif" => Ok(office_ir::ImageFormat::Gif),
        "tif" | "tiff" => Ok(office_ir::ImageFormat::Tiff),
        "bmp" => Ok(office_ir::ImageFormat::Bmp),
        "emf" => Ok(office_ir::ImageFormat::Emf),
        "wmf" => Ok(office_ir::ImageFormat::Wmf),
        _ => bail!("unsupported image format for {path}"),
    }
}

fn add_notes_to_pptx(
    bytes: Vec<u8>,
    notes: &[Option<String>],
    slide_count: usize,
) -> Result<Vec<u8>> {
    let mut source = ZipArchive::new(Cursor::new(bytes)).context("read generated PPTX")?;
    let mut output = Cursor::new(Vec::new());
    let mut writer = ZipWriter::new(&mut output);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    for idx in 0..source.len() {
        let mut file = source.by_index(idx).context("read PPTX entry")?;
        let name = file.name().to_string();
        if name == "[Content_Types].xml"
            || (1..=slide_count).any(|slide| name == slide_rels_path(slide))
        {
            continue;
        }
        let mut data = Vec::new();
        file.read_to_end(&mut data)
            .context("read PPTX entry data")?;
        writer
            .start_file(name.clone(), options)
            .context("write PPTX entry")?;
        writer.write_all(&data).context("write PPTX entry data")?;
    }

    let content_types = read_zip_string(&mut source, "[Content_Types].xml")?;
    writer.start_file("[Content_Types].xml", options)?;
    writer.write_all(add_note_content_types(content_types, notes).as_bytes())?;

    for slide in 1..=slide_count {
        let rels_path = slide_rels_path(slide);
        let rels = read_zip_string(&mut source, &rels_path)?;
        writer.start_file(rels_path, options)?;
        if notes.get(slide - 1).and_then(Option::as_ref).is_some() {
            writer.write_all(add_note_relationship(rels, slide).as_bytes())?;
        } else {
            writer.write_all(rels.as_bytes())?;
        }
    }

    for (idx, note) in notes.iter().enumerate() {
        let slide = idx + 1;
        if let Some(note) = note {
            let note_path = format!("ppt/notesSlides/notesSlide{slide}.xml");
            writer.start_file(note_path, options)?;
            writer.write_all(notes_xml(note).as_bytes())?;

            let rels_path = format!("ppt/notesSlides/_rels/notesSlide{slide}.xml.rels");
            writer.start_file(rels_path, options)?;
            writer.write_all(notes_rels_xml(slide).as_bytes())?;
        }
    }

    writer.finish().context("finish PPTX with notes")?;
    Ok(output.into_inner())
}

fn read_zip_string<R: Read + Seek>(archive: &mut ZipArchive<R>, path: &str) -> Result<String> {
    let mut file = archive
        .by_name(path)
        .with_context(|| format!("read {path}"))?;
    let mut data = String::new();
    file.read_to_string(&mut data)
        .with_context(|| format!("read {path} text"))?;
    Ok(data)
}

fn add_note_content_types(mut xml: String, notes: &[Option<String>]) -> String {
    let overrides = notes
        .iter()
        .enumerate()
        .filter_map(|(idx, note)| {
            note.as_ref().map(|_| {
                let slide = idx + 1;
                format!(
                    r#"<Override PartName="/ppt/notesSlides/notesSlide{slide}.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.notesSlide+xml"/>"#
                )
            })
        })
        .collect::<String>();
    xml = xml.replace("</Types>", &format!("{overrides}</Types>"));
    xml
}

fn add_note_relationship(mut xml: String, slide: usize) -> String {
    let rid = next_relationship_id(&xml);
    let rel = format!(
        r#"<Relationship Id="rId{rid}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesSlide" Target="../notesSlides/notesSlide{slide}.xml"/>"#
    );
    xml = xml.replace("</Relationships>", &format!("{rel}</Relationships>"));
    xml
}

fn next_relationship_id(xml: &str) -> usize {
    let mut max_id = 0;
    let mut rest = xml;
    while let Some(pos) = rest.find("Id=\"rId") {
        rest = &rest[pos + 7..];
        let digits = rest
            .chars()
            .take_while(|ch| ch.is_ascii_digit())
            .collect::<String>();
        if let Ok(id) = digits.parse::<usize>() {
            max_id = max_id.max(id);
        }
    }
    max_id + 1
}

fn slide_rels_path(slide: usize) -> String {
    format!("ppt/slides/_rels/slide{slide}.xml.rels")
}

fn notes_xml(notes: &str) -> String {
    let paragraphs = notes
        .lines()
        .map(|line| format!("<a:p><a:r><a:t>{}</a:t></a:r></a:p>", xml_escape(line)))
        .collect::<String>();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:notes xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/><p:sp><p:nvSpPr><p:cNvPr id="2" name="Slide Image"/><p:cNvSpPr/><p:nvPr><p:ph type="sldImg"/></p:nvPr></p:nvSpPr><p:spPr/></p:sp><p:sp><p:nvSpPr><p:cNvPr id="3" name="Notes Placeholder"/><p:cNvSpPr/><p:nvPr><p:ph type="body" idx="1"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/>{paragraphs}</p:txBody></p:sp></p:spTree></p:cSld></p:notes>"#
    )
}

fn notes_rels_xml(slide: usize) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="../slides/slide{slide}.xml"/></Relationships>"#
    )
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn xml_attr(e: &BytesStart<'_>, key: &[u8], decoder: Decoder) -> Result<Option<String>> {
    for attr in e.attributes() {
        let attr = attr.context("parse XML attribute")?;
        if attr.key.as_ref() == key {
            return Ok(Some(
                attr.decoded_and_normalized_value(XmlVersion::Implicit1_0, decoder)?
                    .into_owned(),
            ));
        }
    }
    Ok(None)
}

fn required_attr(e: &BytesStart<'_>, key: &[u8], decoder: Decoder) -> Result<String> {
    let key_name = std::str::from_utf8(key).unwrap_or("attribute");
    xml_attr(e, key, decoder)?.ok_or_else(|| anyhow::anyhow!("missing {key_name}"))
}

fn optional_f64_attr(e: &BytesStart<'_>, key: &[u8], decoder: Decoder) -> Result<Option<f64>> {
    let Some(value) = xml_attr(e, key, decoder)? else {
        return Ok(None);
    };
    let key_name = std::str::from_utf8(key).unwrap_or("attribute");
    let parsed = value
        .parse::<f64>()
        .with_context(|| format!("invalid {key_name}"))?;
    if parsed < 0.0 {
        bail!("{key_name} must be non-negative");
    }
    Ok(Some(parsed))
}

fn required_f64_attr(e: &BytesStart<'_>, key: &[u8], decoder: Decoder) -> Result<f64> {
    let key_name = std::str::from_utf8(key).unwrap_or("attribute");
    let value = xml_attr(e, key, decoder)?.ok_or_else(|| anyhow::anyhow!("missing {key_name}"))?;
    let parsed = value
        .parse::<f64>()
        .with_context(|| format!("invalid {key_name}"))?;
    if parsed < 0.0 {
        bail!("{key_name} must be non-negative");
    }
    Ok(parsed)
}

fn inches_to_emu(value: f64) -> i64 {
    (value * 914_400.0).round() as i64
}

fn inches_to_emu_u64(value: f64) -> u64 {
    (value * 914_400.0).round() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presentation_xml_image_paths_lists_workspace_images() {
        let paths = image_paths(
            r#"<presentation><slide><image src="/a.png"/><columns><left><image src="/b.jpg"/></left><right><image src="/a.png"/></right></columns></slide></presentation>"#,
        )
        .unwrap();

        assert_eq!(paths, vec!["/a.png", "/b.jpg"]);
    }

    #[test]
    fn presentation_notes_do_not_create_dangling_relationships() {
        let bytes = render(
            r#"
            <presentation>
              <slide><title>With notes</title><notes>Only this slide has notes.</notes></slide>
              <slide><title>No notes</title></slide>
            </presentation>
            "#,
            Vec::new(),
        )
        .unwrap();

        let mut zip = ZipArchive::new(Cursor::new(bytes)).unwrap();
        assert!(zip.by_name("ppt/notesSlides/notesSlide1.xml").is_ok());
        assert!(zip.by_name("ppt/notesSlides/notesSlide2.xml").is_err());

        let slide1_rels = read_zip_string(&mut zip, "ppt/slides/_rels/slide1.xml.rels").unwrap();
        let slide2_rels = read_zip_string(&mut zip, "ppt/slides/_rels/slide2.xml.rels").unwrap();
        assert!(slide1_rels.contains("notesSlide1.xml"));
        assert!(!slide2_rels.contains("notesSlide"));
    }

    #[test]
    fn presentation_xml_flattens_inline_text_markup() {
        let xml = r#"
            <presentation>
              <slide>
                <title>The <b>Problem</b></title>
                <bullets><item><b>Target:</b> embedded processors</item></bullets>
                <textbox x="1" y="2" w="5" h="1"><b>Example Code:</b><br/>int x = 1;</textbox>
              </slide>
            </presentation>
            "#;
        let parsed = parse_presentation_xml(xml).unwrap();
        assert_eq!(parsed.slides[0].title.as_deref(), Some("The Problem"));
        match &parsed.slides[0].items[0] {
            PresentationItem::Bullets(items) => {
                assert_eq!(items[0], "Target: embedded processors");
            }
            _ => panic!("expected bullets"),
        }
        match &parsed.slides[0].items[1] {
            PresentationItem::TextBox { text, .. } => {
                assert_eq!(text, "Example Code:\nint x = 1;");
            }
            _ => panic!("expected textbox"),
        }

        let bytes = render(xml, Vec::new()).unwrap();
        assert!(bytes.starts_with(b"PK"));
    }
}
