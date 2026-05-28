use std::io::Cursor;

use anyhow::{Context, Result, bail};
use comrak::{
    Arena, Options,
    nodes::{AstNode, ListType, NodeValue},
    parse_document,
};
use futures::channel::oneshot;
use office_oxide::{Document, DocumentFormat, create::create_from_ir_to_writer, ir as office_ir};
use pdf_oxide::{
    PdfDocument, converters::ConversionOptions, writer::DocumentBuilder, writer::FluentPageBuilder,
};

const ASYNC_THRESHOLD: usize = 1024 * 1024;
const PAGE_LEFT: f32 = 58.0;
const PAGE_RIGHT: f32 = 58.0;
const PAGE_TOP: f32 = 734.0;
const PAGE_BOTTOM: f32 = 58.0;
const PAGE_WIDTH: f32 = 612.0;
const BODY_SIZE: f32 = 10.5;
const BODY_LINE_HEIGHT: f32 = 14.6;
const TABLE_SIZE: f32 = 8.2;
const TABLE_LINE_HEIGHT: f32 = 11.0;
const TABLE_CELL_PADDING: f32 = 4.0;
const LINK_COLOR: (f32, f32, f32) = (0.03, 0.24, 0.55);

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
    let arena = Arena::new();
    let root = parse_document(&arena, markdown, &markdown_options());
    let mut builder = DocumentBuilder::new();
    PdfRenderer::new(builder.letter_page().at(PAGE_LEFT, PAGE_TOP))
        .render_children(root, BlockContext::default())
        .finish();
    builder.build().context("write PDF")
}

fn markdown_options() -> Options<'static> {
    let mut options = Options::default();
    options.extension.table = true;
    options.extension.strikethrough = true;
    options.extension.autolink = true;
    options.extension.tasklist = true;
    options.parse.relaxed_autolinks = true;
    options
}

#[derive(Clone, Copy, Default)]
struct BlockContext {
    indent: f32,
}

#[derive(Clone, Default)]
struct InlineStyle {
    bold: bool,
    italic: bool,
    code: bool,
    strikethrough: bool,
    link: Option<String>,
}

#[derive(Clone)]
struct InlineRun {
    text: String,
    style: InlineStyle,
}

struct PdfTableRow {
    cells: Vec<Vec<InlineRun>>,
    is_header: bool,
}

struct PdfRenderer<'a> {
    page: FluentPageBuilder<'a>,
    y: f32,
}

impl<'a> PdfRenderer<'a> {
    fn new(page: FluentPageBuilder<'a>) -> Self {
        Self { page, y: PAGE_TOP }
    }

    fn finish(self) {
        self.page.done();
    }

    fn render_children(mut self, node: &'a AstNode<'a>, ctx: BlockContext) -> Self {
        for child in node.children() {
            self = self.render_block(child, ctx);
        }
        self
    }

    fn render_block(mut self, node: &'a AstNode<'a>, ctx: BlockContext) -> Self {
        let value = node.data().value.clone();
        match value {
            NodeValue::Document => self.render_children(node, ctx),
            NodeValue::Paragraph => {
                let runs = inline_runs(node, InlineStyle::default());
                self.render_runs(&runs, ctx.indent, BODY_SIZE, BODY_LINE_HEIGHT, 7.0)
            }
            NodeValue::Heading(heading) => {
                let size = heading_size(heading.level);
                let style = InlineStyle {
                    bold: true,
                    ..Default::default()
                };
                let runs = inline_runs(node, style);
                self = self.add_space(if heading.level == 1 { 2.0 } else { 4.0 });
                self.render_runs(&runs, ctx.indent, size, size * 1.18, 8.0)
            }
            NodeValue::List(list) => self.render_list(node, ctx, list.list_type, list.start),
            NodeValue::BlockQuote | NodeValue::MultilineBlockQuote(_) => {
                let line_x = PAGE_LEFT + ctx.indent + 3.0;
                let start_y = self.y + 4.0;
                self = self.render_children(
                    node,
                    BlockContext {
                        indent: ctx.indent + 16.0,
                    },
                );
                let end_y = self.y + 6.0;
                self.page = self.page.line(line_x, start_y, line_x, end_y);
                self.add_space(2.0)
            }
            NodeValue::CodeBlock(code) => {
                self.render_code_block(ctx.indent, &code.info, code.literal.trim_end_matches('\n'))
            }
            NodeValue::ThematicBreak => self.render_rule(ctx.indent),
            NodeValue::Table(_) => self.render_table(node, ctx),
            NodeValue::TableRow(_) | NodeValue::TableCell | NodeValue::Item(_) => {
                self.render_children(node, ctx)
            }
            NodeValue::TaskItem(task) => {
                let text = if task.symbol.is_some() {
                    "[x] "
                } else {
                    "[ ] "
                };
                let mut runs = vec![InlineRun {
                    text: text.to_string(),
                    style: InlineStyle::default(),
                }];
                runs.extend(inline_runs(node, InlineStyle::default()));
                self.render_runs(&runs, ctx.indent, BODY_SIZE, BODY_LINE_HEIGHT, 4.0)
            }
            _ => self.render_children(node, ctx),
        }
    }

    fn render_list(
        mut self,
        node: &'a AstNode<'a>,
        ctx: BlockContext,
        list_type: ListType,
        start: usize,
    ) -> Self {
        for (index, item) in (start.max(1)..).zip(node.children()) {
            self = self.ensure_line(BODY_LINE_HEIGHT);
            let marker = match list_type {
                ListType::Bullet => "-".to_string(),
                ListType::Ordered => format!("{index}."),
            };
            let y = self.y;
            self = self.emit_text(
                PAGE_LEFT + ctx.indent,
                y,
                &marker,
                InlineStyle::default(),
                BODY_SIZE,
            );
            self = self.render_children(
                item,
                BlockContext {
                    indent: ctx.indent + 24.0,
                },
            );
        }
        self.add_space(2.0)
    }

    fn render_table(mut self, node: &'a AstNode<'a>, ctx: BlockContext) -> Self {
        let rows = pdf_table_rows(node);
        let col_count = rows.iter().map(|row| row.cells.len()).max().unwrap_or(0);
        if col_count == 0 {
            return self;
        }

        let x = PAGE_LEFT + ctx.indent;
        let total_width = PAGE_WIDTH - PAGE_RIGHT - x;
        let widths = table_column_widths(&rows, total_width, col_count);
        let header = rows.iter().find(|row| row.is_header);

        self = self.add_space(2.0);
        for (idx, row) in rows.iter().enumerate() {
            let measured = self.measure_table_row(row, &widths);
            self = measured.0;
            let wrapped = measured.1;
            let row_height = measured.2;

            if self.y - row_height < PAGE_BOTTOM {
                self.page = self.page.new_page_same_size().at(PAGE_LEFT, PAGE_TOP);
                self.y = PAGE_TOP;
                if idx > 0
                    && !row.is_header
                    && let Some(header_row) = header
                {
                    let measured = self.measure_table_row(header_row, &widths);
                    self = measured.0;
                    self = self.draw_table_row(x, &widths, measured.1, measured.2, true);
                }
            }

            self = self.draw_table_row(x, &widths, wrapped, row_height, row.is_header);
        }
        self.add_space(8.0)
    }

    fn measure_table_row(
        mut self,
        row: &PdfTableRow,
        widths: &[f32],
    ) -> (Self, Vec<Vec<Vec<InlineRun>>>, f32) {
        let mut wrapped_cells = Vec::with_capacity(widths.len());
        let mut row_height = TABLE_LINE_HEIGHT + TABLE_CELL_PADDING * 2.0;

        for (idx, width) in widths.iter().enumerate() {
            let empty = Vec::new();
            let runs = row.cells.get(idx).unwrap_or(&empty);
            let measured = self.wrap_runs(runs, width - TABLE_CELL_PADDING * 2.0, TABLE_SIZE);
            self = measured.0;
            let lines = if measured.1.is_empty() {
                vec![Vec::new()]
            } else {
                measured.1
            };
            row_height =
                row_height.max(lines.len() as f32 * TABLE_LINE_HEIGHT + TABLE_CELL_PADDING * 2.0);
            wrapped_cells.push(lines);
        }

        (self, wrapped_cells, row_height)
    }

    fn draw_table_row(
        mut self,
        x: f32,
        widths: &[f32],
        wrapped_cells: Vec<Vec<Vec<InlineRun>>>,
        row_height: f32,
        is_header: bool,
    ) -> Self {
        let top = self.y;
        let bottom = top - row_height;
        let total_width: f32 = widths.iter().sum();

        if is_header {
            self.page = self
                .page
                .filled_rect(x, bottom, total_width, row_height, 0.93, 0.94, 0.95);
        }

        self.page = self.page.line(x, top, x + total_width, top);
        self.page = self.page.line(x, bottom, x + total_width, bottom);

        let mut cell_x = x;
        for (idx, width) in widths.iter().enumerate() {
            self.page = self.page.line(cell_x, top, cell_x, bottom);
            let mut line_y = top - TABLE_CELL_PADDING - TABLE_SIZE;
            for line in wrapped_cells.get(idx).into_iter().flatten() {
                let mut run_x = cell_x + TABLE_CELL_PADDING;
                for run in line {
                    let mut style = run.style.clone();
                    style.bold |= is_header;
                    let measured = self.measure_text(&run.text, &style, TABLE_SIZE);
                    self = measured.0;
                    let width = measured.1;
                    self = self.emit_text(run_x, line_y, &run.text, style, TABLE_SIZE);
                    run_x += width;
                }
                line_y -= TABLE_LINE_HEIGHT;
            }
            cell_x += width;
        }
        self.page = self.page.line(cell_x, top, cell_x, bottom);

        self.y = bottom;
        self
    }

    fn render_code_block(mut self, indent: f32, language: &str, source: &str) -> Self {
        if !language.trim().is_empty() {
            let style = InlineStyle {
                code: true,
                bold: true,
                ..Default::default()
            };
            let runs = vec![InlineRun {
                text: language.trim().to_string(),
                style,
            }];
            self = self.render_runs(&runs, indent, 8.5, 11.0, 2.0);
        }

        let x = PAGE_LEFT + indent;
        let width = PAGE_WIDTH - x - PAGE_RIGHT;
        for line in source.lines().chain((source.is_empty()).then_some("")) {
            self = self.ensure_line(13.0);
            self.page =
                self.page
                    .filled_rect(x - 4.0, self.y - 3.0, width + 8.0, 13.0, 0.96, 0.96, 0.94);
            let style = InlineStyle {
                code: true,
                ..Default::default()
            };
            let y = self.y;
            self = self.emit_text(x, y, line, style, 8.8);
            self.y -= 12.0;
        }
        self.add_space(7.0)
    }

    fn render_rule(mut self, indent: f32) -> Self {
        self = self.ensure_line(16.0);
        let x = PAGE_LEFT + indent;
        self.page = self.page.line(x, self.y, PAGE_WIDTH - PAGE_RIGHT, self.y);
        self.y -= 12.0;
        self
    }

    fn render_runs(
        mut self,
        runs: &[InlineRun],
        indent: f32,
        size: f32,
        line_height: f32,
        after_space: f32,
    ) -> Self {
        let start_x = PAGE_LEFT + indent;
        let max_right = PAGE_WIDTH - PAGE_RIGHT;
        let mut x = start_x;
        let mut line_started = false;

        self = self.ensure_line(line_height);
        for run in runs {
            for token in split_tokens(&run.text) {
                if token == "\n" {
                    if line_started {
                        self.y -= line_height;
                    }
                    self = self.ensure_line(line_height);
                    x = start_x;
                    line_started = false;
                    continue;
                }

                let token = if token.chars().all(char::is_whitespace) {
                    " "
                } else {
                    token.as_str()
                };
                if token == " " && !line_started {
                    continue;
                }

                let measured = self.measure_text(token, &run.style, size);
                self = measured.0;
                let width = measured.1;
                if x + width > max_right && line_started {
                    self.y -= line_height;
                    self = self.ensure_line(line_height);
                    x = start_x;
                    line_started = false;
                    if token == " " {
                        continue;
                    }
                }

                let y = self.y;
                self = self.emit_text(x, y, token, run.style.clone(), size);
                x += width;
                line_started = true;
            }
        }

        if line_started {
            self.y -= line_height;
        }
        self.add_space(after_space)
    }

    fn measure_text(mut self, text: &str, style: &InlineStyle, size: f32) -> (Self, f32) {
        self.page = self.page.font(font_for(style), size);
        let width = self.page.measure(text);
        (self, width)
    }

    fn wrap_runs(
        mut self,
        runs: &[InlineRun],
        max_width: f32,
        size: f32,
    ) -> (Self, Vec<Vec<InlineRun>>) {
        let mut lines = Vec::new();
        let mut line = Vec::new();
        let mut x = 0.0;
        let mut line_started = false;

        for run in runs {
            for token in split_tokens(&run.text) {
                if token == "\n" {
                    if !line.is_empty() {
                        lines.push(std::mem::take(&mut line));
                    }
                    x = 0.0;
                    line_started = false;
                    continue;
                }

                let token = if token.chars().all(char::is_whitespace) {
                    " "
                } else {
                    token.as_str()
                };
                if token == " " && !line_started {
                    continue;
                }

                let measured = self.measure_text(token, &run.style, size);
                self = measured.0;
                let width = measured.1;
                if x + width > max_width && line_started {
                    lines.push(std::mem::take(&mut line));
                    x = 0.0;
                    line_started = false;
                    if token == " " {
                        continue;
                    }
                }

                push_run(&mut line, token.to_string(), run.style.clone());
                x += width;
                line_started = true;
            }
        }

        if !line.is_empty() {
            lines.push(line);
        }

        (self, lines)
    }

    fn emit_text(mut self, x: f32, y: f32, text: &str, style: InlineStyle, size: f32) -> Self {
        if text.is_empty() {
            return self;
        }

        let font = font_for(&style);
        self.page = self.page.at(x, y).font(font, size);
        if style.code {
            let width = self.page.measure(text);
            self.page = self
                .page
                .filled_rect(x - 1.5, y - 2.0, width + 3.0, size + 3.0, 0.95, 0.95, 0.93)
                .at(x, y)
                .font(font, size);
        }

        if let Some(url) = style.link {
            self.page = self
                .page
                .inline_color(LINK_COLOR.0, LINK_COLOR.1, LINK_COLOR.2, text)
                .underline(LINK_COLOR)
                .link_url(&url);
        } else {
            self.page = self.page.inline(text);
        }
        if style.strikethrough {
            self.page = self.page.strikeout((0.0, 0.0, 0.0));
        }
        self
    }

    fn ensure_line(mut self, line_height: f32) -> Self {
        if self.y - line_height < PAGE_BOTTOM {
            self.page = self.page.new_page_same_size().at(PAGE_LEFT, PAGE_TOP);
            self.y = PAGE_TOP;
        }
        self
    }

    fn add_space(mut self, points: f32) -> Self {
        self.y -= points;
        self
    }
}

fn inline_runs<'a>(node: &'a AstNode<'a>, style: InlineStyle) -> Vec<InlineRun> {
    let mut runs = Vec::new();
    collect_inline_runs(node, style, &mut runs);
    runs
}

fn collect_inline_runs<'a>(node: &'a AstNode<'a>, style: InlineStyle, runs: &mut Vec<InlineRun>) {
    match node.data().value.clone() {
        NodeValue::Text(text) => push_run(runs, text.into_owned(), style),
        NodeValue::Code(code) => {
            let mut code_style = style;
            code_style.code = true;
            push_run(runs, code.literal, code_style);
        }
        NodeValue::SoftBreak => push_run(runs, " ".to_string(), style),
        NodeValue::LineBreak => push_run(runs, "\n".to_string(), style),
        NodeValue::Strong => {
            let mut nested = style;
            nested.bold = true;
            collect_children_inline(node, nested, runs);
        }
        NodeValue::Emph => {
            let mut nested = style;
            nested.italic = true;
            collect_children_inline(node, nested, runs);
        }
        NodeValue::Strikethrough => {
            let mut nested = style;
            nested.strikethrough = true;
            collect_children_inline(node, nested, runs);
        }
        NodeValue::Link(link) => {
            let mut nested = style;
            nested.link = Some(link.url);
            let before = runs.len();
            collect_children_inline(node, nested.clone(), runs);
            if runs.len() == before {
                push_run(runs, nested.link.clone().unwrap_or_default(), nested);
            }
        }
        NodeValue::Image(link) => {
            let alt = plain_text(node);
            let mut nested = style;
            nested.italic = true;
            nested.link = Some(link.url);
            push_run(
                runs,
                if alt.is_empty() {
                    "[image]".to_string()
                } else {
                    format!("[image: {alt}]")
                },
                nested,
            );
        }
        NodeValue::HtmlInline(html) | NodeValue::Raw(html) => push_run(runs, html, style),
        _ => collect_children_inline(node, style, runs),
    }
}

fn collect_children_inline<'a>(
    node: &'a AstNode<'a>,
    style: InlineStyle,
    runs: &mut Vec<InlineRun>,
) {
    for child in node.children() {
        collect_inline_runs(child, style.clone(), runs);
    }
}

fn push_run(runs: &mut Vec<InlineRun>, text: String, style: InlineStyle) {
    if text.is_empty() {
        return;
    }
    if let Some(last) = runs.last_mut()
        && same_style(&last.style, &style)
    {
        last.text.push_str(&text);
        return;
    }
    runs.push(InlineRun { text, style });
}

fn same_style(a: &InlineStyle, b: &InlineStyle) -> bool {
    a.bold == b.bold
        && a.italic == b.italic
        && a.code == b.code
        && a.strikethrough == b.strikethrough
        && a.link == b.link
}

fn split_tokens(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_whitespace = false;

    for ch in text.chars() {
        if ch == '\n' {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
            tokens.push("\n".to_string());
            in_whitespace = false;
        } else if ch.is_whitespace() {
            if !in_whitespace && !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
            current.push(ch);
            in_whitespace = true;
        } else {
            if in_whitespace && !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
            current.push(ch);
            in_whitespace = false;
        }
    }

    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

fn plain_text<'a>(node: &'a AstNode<'a>) -> String {
    match node.data().value.clone() {
        NodeValue::Text(text) => text.into_owned(),
        NodeValue::Code(code) => code.literal,
        NodeValue::SoftBreak | NodeValue::LineBreak => " ".to_string(),
        _ => node.children().map(plain_text).collect::<Vec<_>>().join(""),
    }
}

fn pdf_table_rows<'a>(node: &'a AstNode<'a>) -> Vec<PdfTableRow> {
    node.children()
        .filter_map(|row| {
            let value = row.data().value.clone();
            let is_header = matches!(value, NodeValue::TableRow(true));
            let cells = row
                .children()
                .map(|cell| inline_runs(cell, InlineStyle::default()))
                .collect::<Vec<_>>();
            (!cells.is_empty()).then_some(PdfTableRow { cells, is_header })
        })
        .collect()
}

fn table_column_widths(rows: &[PdfTableRow], total_width: f32, col_count: usize) -> Vec<f32> {
    let min_width = if col_count > 6 { 30.0 } else { 38.0 };
    let base_width = min_width * col_count as f32;
    if base_width >= total_width {
        return vec![total_width / col_count as f32; col_count];
    }

    let mut weights = vec![5.0_f32; col_count];
    for row in rows {
        for (idx, cell) in row.cells.iter().enumerate() {
            let chars = cell
                .iter()
                .map(|run| run.text.chars().count())
                .sum::<usize>() as f32;
            weights[idx] = weights[idx].max(chars.clamp(5.0, 34.0));
        }
    }

    let remaining = total_width - base_width;
    let total_weight: f32 = weights.iter().sum();
    weights
        .into_iter()
        .map(|weight| min_width + remaining * weight / total_weight)
        .collect()
}

fn font_for(style: &InlineStyle) -> &'static str {
    if style.code {
        if style.bold {
            return "Courier-Bold";
        }
        if style.italic {
            return "Courier-Oblique";
        }
        return "Courier";
    }

    match (style.bold, style.italic) {
        (true, true) => "Helvetica-BoldOblique",
        (true, false) => "Helvetica-Bold",
        (false, true) => "Helvetica-Oblique",
        (false, false) => "Helvetica",
    }
}

fn heading_size(level: u8) -> f32 {
    match level {
        1 => 22.0,
        2 => 17.0,
        3 => 14.0,
        4 => 12.0,
        5 => 11.0,
        _ => 10.5,
    }
}

fn markdown_to_office_word_sync(markdown: &str) -> Result<Vec<u8>> {
    let ir = markdown_to_office_ir(markdown);
    let mut writer = Cursor::new(Vec::new());
    create_from_ir_to_writer(&ir, DocumentFormat::Docx, &mut writer).context("write DOCX")?;
    Ok(writer.into_inner())
}

fn markdown_to_office_ir(markdown: &str) -> office_ir::DocumentIR {
    let arena = Arena::new();
    let root = parse_document(&arena, markdown, &markdown_options());
    let mut elements = Vec::new();
    collect_office_elements(root, 0, &mut elements);

    office_ir::DocumentIR {
        metadata: office_ir::Metadata {
            format: DocumentFormat::Docx,
            ..Default::default()
        },
        sections: vec![office_ir::Section {
            elements,
            ..Default::default()
        }],
    }
}

fn collect_office_elements<'a>(
    node: &'a AstNode<'a>,
    indent_left_twips: i32,
    elements: &mut Vec<office_ir::Element>,
) {
    let value = node.data().value.clone();
    match value {
        NodeValue::Document => {
            for child in node.children() {
                collect_office_elements(child, indent_left_twips, elements);
            }
        }
        NodeValue::Paragraph => {
            let content = office_inline_content(node);
            if !content.is_empty() {
                elements.push(office_ir::Element::Paragraph(office_ir::Paragraph {
                    content,
                    indent_left_twips: (indent_left_twips > 0).then_some(indent_left_twips),
                    ..Default::default()
                }));
            }
        }
        NodeValue::Heading(heading) => {
            let content = office_inline_content(node);
            if !content.is_empty() {
                elements.push(office_ir::Element::Heading(office_ir::Heading {
                    level: heading.level.clamp(1, 6),
                    content,
                    ..Default::default()
                }));
            }
        }
        NodeValue::List(list) => {
            elements.push(office_ir::Element::List(office_ir::List {
                ordered: list.list_type == ListType::Ordered,
                items: node.children().map(office_list_item).collect(),
                start_number: (list.start > 1).then_some(list.start as u32),
                ..Default::default()
            }));
        }
        NodeValue::BlockQuote | NodeValue::MultilineBlockQuote(_) => {
            for child in node.children() {
                collect_office_elements(child, indent_left_twips + 360, elements);
            }
        }
        NodeValue::CodeBlock(code) => {
            elements.push(office_ir::Element::CodeBlock(office_ir::CodeBlock {
                language: (!code.info.trim().is_empty()).then(|| code.info.trim().to_string()),
                content: code.literal,
            }));
        }
        NodeValue::ThematicBreak => elements.push(office_ir::Element::ThematicBreak),
        NodeValue::Table(_) => elements.push(office_ir::Element::Table(office_table(node))),
        NodeValue::TableRow(_) | NodeValue::TableCell | NodeValue::Item(_) => {
            for child in node.children() {
                collect_office_elements(child, indent_left_twips, elements);
            }
        }
        NodeValue::TaskItem(task) => {
            let mut content = vec![office_ir::InlineContent::Text(office_ir::TextSpan::plain(
                if task.symbol.is_some() {
                    "[x] "
                } else {
                    "[ ] "
                },
            ))];
            content.extend(office_inline_content(node));
            elements.push(office_ir::Element::Paragraph(office_ir::Paragraph {
                content,
                indent_left_twips: (indent_left_twips > 0).then_some(indent_left_twips),
                ..Default::default()
            }));
        }
        _ => {
            for child in node.children() {
                collect_office_elements(child, indent_left_twips, elements);
            }
        }
    }
}

fn office_list_item<'a>(node: &'a AstNode<'a>) -> office_ir::ListItem {
    let mut content = Vec::new();
    for child in node.children() {
        collect_office_elements(child, 0, &mut content);
    }
    office_ir::ListItem {
        content,
        ..Default::default()
    }
}

fn office_table<'a>(node: &'a AstNode<'a>) -> office_ir::Table {
    office_ir::Table {
        rows: node
            .children()
            .map(|row| {
                let is_header = matches!(row.data().value, NodeValue::TableRow(true));
                office_ir::TableRow {
                    cells: row
                        .children()
                        .map(|cell| office_ir::TableCell {
                            content: office_ir::inline_to_element_block(office_inline_content(
                                cell,
                            )),
                            col_span: 1,
                            row_span: 1,
                            ..Default::default()
                        })
                        .collect(),
                    is_header,
                    repeat_as_header: is_header,
                    ..Default::default()
                }
            })
            .collect(),
        ..Default::default()
    }
}

fn office_inline_content<'a>(node: &'a AstNode<'a>) -> Vec<office_ir::InlineContent> {
    let mut content = Vec::new();
    for run in inline_runs(node, InlineStyle::default()) {
        for (idx, text) in run.text.split('\n').enumerate() {
            if idx > 0 {
                content.push(office_ir::InlineContent::LineBreak);
            }
            if !text.is_empty() {
                content.push(office_ir::InlineContent::Text(office_text_span(
                    text, &run.style,
                )));
            }
        }
    }
    content
}

fn office_text_span(text: &str, style: &InlineStyle) -> office_ir::TextSpan {
    office_ir::TextSpan {
        text: text.to_string(),
        bold: style.bold,
        italic: style.italic,
        strikethrough: style.strikethrough,
        hyperlink: style.link.clone(),
        font_name: style.code.then(|| "Courier New".to_string()),
        highlight: style.code.then_some([242, 242, 238]),
        color: style.link.as_ref().map(|_| [5, 61, 140]),
        underline: style
            .link
            .as_ref()
            .map(|_| office_ir::UnderlineStyle::Single),
        ..Default::default()
    }
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
}
