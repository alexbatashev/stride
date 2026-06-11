//! Markdown to PDF rendering.
//!
//! Layout happens in two phases: inline runs are first wrapped into lines of
//! style-coalesced segments, then each segment is emitted exactly once. This
//! keeps link/underline/strikeout annotations (which attach to the last
//! emitted text run) covering whole phrases instead of individual words.

use anyhow::{Context, Result};
use comrak::{
    Arena,
    nodes::{AstNode, ListType, NodeList, NodeValue},
    parse_document,
};
use pdf_oxide::writer::{DocumentBuilder, FluentPageBuilder};

use crate::markdown::{self, InlineRun, InlineStyle, task_marker};

const PAGE_LEFT: f32 = 58.0;
const PAGE_RIGHT: f32 = 58.0;
const PAGE_TOP: f32 = 734.0;
const PAGE_BOTTOM: f32 = 58.0;
const PAGE_WIDTH: f32 = 612.0;
const BODY_SIZE: f32 = 10.5;
const BODY_LINE_HEIGHT: f32 = 14.6;
const CODE_SIZE: f32 = 8.8;
const CODE_LINE_HEIGHT: f32 = 12.0;
const TABLE_SIZE: f32 = 8.2;
const TABLE_LINE_HEIGHT: f32 = 11.0;
const TABLE_CELL_PADDING: f32 = 4.0;
const LINK_COLOR: (f32, f32, f32) = (0.03, 0.24, 0.55);

pub fn render(markdown: &str) -> Result<Vec<u8>> {
    let arena = Arena::new();
    let root = parse_document(&arena, markdown, &markdown::options());
    let mut builder = DocumentBuilder::new();
    {
        let mut renderer = Renderer {
            page: Some(builder.letter_page().at(PAGE_LEFT, PAGE_TOP)),
            y: PAGE_TOP,
            breaks: 0,
        };
        renderer.blocks(root, Ctx::default());
        renderer.page.take().expect("page builder").done();
    }
    builder.build().context("write PDF")
}

#[derive(Clone, Copy, Default)]
struct Ctx {
    indent: f32,
    tight: bool,
}

#[derive(Clone)]
struct Segment {
    text: String,
    style: InlineStyle,
    width: f32,
}

struct TableRow {
    cells: Vec<Vec<InlineRun>>,
    is_header: bool,
}

struct Renderer<'a> {
    /// `FluentPageBuilder` methods consume `self`, so it lives in an `Option`
    /// and is threaded through `apply`.
    page: Option<FluentPageBuilder<'a>>,
    y: f32,
    breaks: u32,
}

impl<'a> Renderer<'a> {
    fn apply(&mut self, f: impl FnOnce(FluentPageBuilder<'a>) -> FluentPageBuilder<'a>) {
        let page = self.page.take().expect("page builder");
        self.page = Some(f(page));
    }

    fn measure(&mut self, text: &str, style: &InlineStyle, size: f32) -> f32 {
        let font = font_for(style);
        self.apply(|p| p.font(font, size));
        self.page.as_ref().expect("page builder").measure(text)
    }

    fn break_page(&mut self) {
        self.apply(|p| p.new_page_same_size().at(PAGE_LEFT, PAGE_TOP));
        self.y = PAGE_TOP;
        self.breaks += 1;
    }

    fn ensure_line(&mut self, line_height: f32) {
        if self.y - line_height < PAGE_BOTTOM {
            self.break_page();
        }
    }

    fn blocks(&mut self, node: &'a AstNode<'a>, ctx: Ctx) {
        for child in node.children() {
            self.block(child, ctx);
        }
    }

    fn block(&mut self, node: &'a AstNode<'a>, ctx: Ctx) {
        match node.data().value.clone() {
            NodeValue::Paragraph => {
                let runs = markdown::inline_runs(node, InlineStyle::default());
                let after = if ctx.tight { 2.0 } else { 7.0 };
                self.paragraph(&runs, ctx, BODY_SIZE, BODY_LINE_HEIGHT, after);
            }
            NodeValue::Heading(heading) => self.heading(node, ctx, heading.level),
            NodeValue::List(list) => self.list(node, ctx, &list),
            NodeValue::BlockQuote | NodeValue::MultilineBlockQuote(_) => {
                self.block_quote(node, ctx)
            }
            NodeValue::CodeBlock(code) => {
                self.code_block(ctx, &code.info, code.literal.trim_end_matches('\n'))
            }
            NodeValue::ThematicBreak => self.rule(ctx),
            NodeValue::Table(_) => self.table(node, ctx),
            _ => self.blocks(node, ctx),
        }
    }

    fn paragraph(&mut self, runs: &[InlineRun], ctx: Ctx, size: f32, line_height: f32, after: f32) {
        let x = PAGE_LEFT + ctx.indent;
        let lines = self.wrap(runs, PAGE_WIDTH - PAGE_RIGHT - x, size);
        for line in &lines {
            self.ensure_line(line_height);
            self.draw_line(x, line, size);
            self.y -= line_height;
        }
        self.y -= after;
    }

    fn heading(&mut self, node: &'a AstNode<'a>, ctx: Ctx, level: u8) {
        let size = heading_size(level);
        let style = InlineStyle {
            bold: true,
            ..Default::default()
        };
        let runs = markdown::inline_runs(node, style);
        self.y -= if level == 1 { 2.0 } else { 4.0 };
        // keep the heading attached to at least one body line
        self.ensure_line(size * 1.18 + BODY_LINE_HEIGHT);
        self.paragraph(&runs, ctx, size, size * 1.18, 8.0);
    }

    fn list(&mut self, node: &'a AstNode<'a>, ctx: Ctx, list: &NodeList) {
        let start = list.start.max(1);
        let marker = |index: usize| match list.list_type {
            ListType::Bullet => "-".to_string(),
            ListType::Ordered => format!("{index}."),
        };
        // hanging indent sized for the widest marker so wrapped lines align
        let last = start + node.children().count().saturating_sub(1);
        let marker_width = self.measure(&marker(last), &InlineStyle::default(), BODY_SIZE);
        let inner = Ctx {
            indent: ctx.indent + (marker_width + 6.0).max(14.0),
            tight: list.tight,
        };

        for (index, item) in (start..).zip(node.children()) {
            self.ensure_line(BODY_LINE_HEIGHT);
            let text = marker(index);
            let width = self.measure(&text, &InlineStyle::default(), BODY_SIZE);
            let segment = Segment {
                text,
                style: InlineStyle::default(),
                width,
            };
            self.emit(PAGE_LEFT + ctx.indent, self.y, &segment, BODY_SIZE);
            let before = (self.y, self.breaks);
            self.list_item(item, inner);
            if (self.y, self.breaks) == before {
                self.y -= BODY_LINE_HEIGHT;
            }
        }
        self.y -= if ctx.tight { 1.0 } else { 4.0 };
    }

    fn list_item(&mut self, item: &'a AstNode<'a>, ctx: Ctx) {
        let mut children = item.children().peekable();
        if let NodeValue::TaskItem(task) = &item.data().value {
            let mut runs = vec![InlineRun {
                text: task_marker(task.symbol).to_string(),
                style: InlineStyle::default(),
            }];
            if let Some(first) = children.peek()
                && matches!(first.data().value, NodeValue::Paragraph)
            {
                runs.extend(markdown::inline_runs(
                    children.next().expect("peeked"),
                    InlineStyle::default(),
                ));
            }
            let after = if ctx.tight { 2.0 } else { 7.0 };
            self.paragraph(&runs, ctx, BODY_SIZE, BODY_LINE_HEIGHT, after);
        }
        for child in children {
            self.block(child, ctx);
        }
    }

    fn block_quote(&mut self, node: &'a AstNode<'a>, ctx: Ctx) {
        let bar_x = PAGE_LEFT + ctx.indent + 3.0;
        let start_y = self.y + 4.0;
        let breaks = self.breaks;
        self.blocks(
            node,
            Ctx {
                indent: ctx.indent + 16.0,
                ..ctx
            },
        );
        // after a page break the quote's start lies on an earlier page; the
        // bar can only be drawn on the current one
        let top = if self.breaks > breaks {
            PAGE_TOP
        } else {
            start_y
        };
        let bottom = self.y + 6.0;
        if top > bottom {
            self.apply(|p| p.line(bar_x, top, bar_x, bottom));
        }
        self.y -= 2.0;
    }

    fn code_block(&mut self, ctx: Ctx, language: &str, source: &str) {
        let style = InlineStyle {
            code: true,
            ..Default::default()
        };
        if !language.trim().is_empty() {
            let runs = [InlineRun {
                text: language.trim().to_string(),
                style: InlineStyle {
                    bold: true,
                    ..style.clone()
                },
            }];
            self.paragraph(&runs, ctx, 8.5, 11.0, 2.0);
        }

        let x = PAGE_LEFT + ctx.indent;
        let width = PAGE_WIDTH - PAGE_RIGHT - x;
        // Courier is monospace, so wrapping is a character count
        let char_width = self.measure("M", &style, CODE_SIZE);
        let max_chars = (((width - 8.0) / char_width) as usize).max(1);

        let lines = source
            .lines()
            .flat_map(|line| split_chars(&line.replace('\t', "    "), max_chars))
            .collect::<Vec<_>>();
        for line in if lines.is_empty() {
            vec![String::new()]
        } else {
            lines
        } {
            self.ensure_line(CODE_LINE_HEIGHT + 1.0);
            let y = self.y;
            self.apply(|p| {
                p.filled_rect(
                    x - 4.0,
                    y - 3.0,
                    width + 8.0,
                    CODE_LINE_HEIGHT + 1.0,
                    0.96,
                    0.96,
                    0.94,
                )
            });
            if !line.is_empty() {
                let font = font_for(&style);
                self.apply(|p| p.at(x, y).font(font, CODE_SIZE).inline(&line));
            }
            self.y -= CODE_LINE_HEIGHT;
        }
        self.y -= 7.0;
    }

    fn rule(&mut self, ctx: Ctx) {
        self.ensure_line(16.0);
        let x = PAGE_LEFT + ctx.indent;
        let y = self.y;
        self.apply(|p| p.line(x, y, PAGE_WIDTH - PAGE_RIGHT, y));
        self.y -= 12.0;
    }

    fn table(&mut self, node: &'a AstNode<'a>, ctx: Ctx) {
        let rows = table_rows(node);
        let col_count = rows.iter().map(|row| row.cells.len()).max().unwrap_or(0);
        if col_count == 0 {
            return;
        }

        let x = PAGE_LEFT + ctx.indent;
        let widths = column_widths(&rows, PAGE_WIDTH - PAGE_RIGHT - x, col_count);
        let header = rows.iter().position(|row| row.is_header);

        self.y -= 2.0;
        for (idx, row) in rows.iter().enumerate() {
            let (cells, height) = self.wrap_row(row, &widths);
            if self.y - height < PAGE_BOTTOM {
                self.break_page();
                if idx > 0
                    && !row.is_header
                    && let Some(header_idx) = header
                {
                    let (cells, height) = self.wrap_row(&rows[header_idx], &widths);
                    self.draw_row(x, &widths, &cells, height, true);
                }
            }
            self.draw_row(x, &widths, &cells, height, row.is_header);
        }
        self.y -= 8.0;
    }

    fn wrap_row(&mut self, row: &TableRow, widths: &[f32]) -> (Vec<Vec<Vec<Segment>>>, f32) {
        let mut cells = Vec::with_capacity(widths.len());
        let mut height = TABLE_LINE_HEIGHT + TABLE_CELL_PADDING * 2.0;

        for (idx, width) in widths.iter().enumerate() {
            let runs = row.cells.get(idx).map(Vec::as_slice).unwrap_or(&[]);
            // headers are bold; bake it in before measuring so widths match
            let runs = if row.is_header {
                runs.iter()
                    .map(|run| InlineRun {
                        text: run.text.clone(),
                        style: InlineStyle {
                            bold: true,
                            ..run.style.clone()
                        },
                    })
                    .collect::<Vec<_>>()
            } else {
                runs.to_vec()
            };
            let lines = self.wrap(&runs, width - TABLE_CELL_PADDING * 2.0, TABLE_SIZE);
            height = height
                .max(lines.len().max(1) as f32 * TABLE_LINE_HEIGHT + TABLE_CELL_PADDING * 2.0);
            cells.push(lines);
        }

        (cells, height)
    }

    fn draw_row(
        &mut self,
        x: f32,
        widths: &[f32],
        cells: &[Vec<Vec<Segment>>],
        height: f32,
        shaded: bool,
    ) {
        let top = self.y;
        let bottom = top - height;
        let total_width: f32 = widths.iter().sum();

        if shaded {
            self.apply(|p| p.filled_rect(x, bottom, total_width, height, 0.93, 0.94, 0.95));
        }
        self.apply(|p| {
            p.line(x, top, x + total_width, top)
                .line(x, bottom, x + total_width, bottom)
        });

        let mut cell_x = x;
        for (idx, width) in widths.iter().enumerate() {
            self.apply(|p| p.line(cell_x, top, cell_x, bottom));
            let mut line_y = top - TABLE_CELL_PADDING - TABLE_SIZE;
            for line in &cells[idx] {
                let mut run_x = cell_x + TABLE_CELL_PADDING;
                for segment in line {
                    self.emit(run_x, line_y, segment, TABLE_SIZE);
                    run_x += segment.width;
                }
                line_y -= TABLE_LINE_HEIGHT;
            }
            cell_x += width;
        }
        self.apply(|p| p.line(cell_x, top, cell_x, bottom));

        self.y = bottom;
    }

    fn draw_line(&mut self, x: f32, line: &[Segment], size: f32) {
        let mut cursor = x;
        for segment in line {
            self.emit(cursor, self.y, segment, size);
            cursor += segment.width;
        }
    }

    fn emit(&mut self, x: f32, y: f32, segment: &Segment, size: f32) {
        if segment.text.is_empty() {
            return;
        }
        self.apply(|mut p| {
            if segment.style.code {
                p = p.filled_rect(
                    x - 1.5,
                    y - 2.0,
                    segment.width + 3.0,
                    size + 3.0,
                    0.95,
                    0.95,
                    0.93,
                );
            }
            p = p.at(x, y).font(font_for(&segment.style), size);
            if let Some(url) = &segment.style.link {
                p = p
                    .inline_color(LINK_COLOR.0, LINK_COLOR.1, LINK_COLOR.2, &segment.text)
                    .underline(LINK_COLOR)
                    .link_url(url);
            } else {
                p = p.inline(&segment.text);
            }
            if segment.style.strikethrough {
                p = p.strikeout((0.0, 0.0, 0.0));
            }
            p
        });
    }

    fn wrap(&mut self, runs: &[InlineRun], max_width: f32, size: f32) -> Vec<Vec<Segment>> {
        wrap_runs(runs, max_width, &mut |text, style| {
            self.measure(text, style, size)
        })
    }
}

/// Greedy word wrap producing lines of style-coalesced segments. Whitespace
/// collapses to single spaces and never starts or ends a line; tokens wider
/// than a full line break at character level.
fn wrap_runs(
    runs: &[InlineRun],
    max_width: f32,
    measure: &mut dyn FnMut(&str, &InlineStyle) -> f32,
) -> Vec<Vec<Segment>> {
    let mut lines = Vec::new();
    let mut line: Vec<Segment> = Vec::new();
    let mut x = 0.0_f32;
    let mut space: Option<Segment> = None;

    for run in runs {
        for token in tokens(&run.text) {
            match token {
                Token::Newline => {
                    lines.push(std::mem::take(&mut line));
                    x = 0.0;
                    space = None;
                }
                Token::Space => {
                    if !line.is_empty() {
                        space = Some(Segment {
                            text: " ".to_string(),
                            width: measure(" ", &run.style),
                            style: run.style.clone(),
                        });
                    }
                }
                Token::Word(word) => {
                    let width = measure(word, &run.style);
                    let space_width = space.as_ref().map_or(0.0, |s| s.width);
                    if !line.is_empty() && x + space_width + width > max_width {
                        lines.push(std::mem::take(&mut line));
                        x = 0.0;
                        space = None;
                    }
                    if let Some(space) = space.take() {
                        x += space.width;
                        push_segment(&mut line, space);
                    }
                    if width > max_width {
                        let mut piece = String::new();
                        let mut piece_width = 0.0;
                        for ch in word.chars() {
                            let mut buf = [0u8; 4];
                            let ch_width = measure(ch.encode_utf8(&mut buf), &run.style);
                            if x + piece_width + ch_width > max_width
                                && !(line.is_empty() && piece.is_empty())
                            {
                                push_segment(
                                    &mut line,
                                    Segment {
                                        text: std::mem::take(&mut piece),
                                        style: run.style.clone(),
                                        width: piece_width,
                                    },
                                );
                                lines.push(std::mem::take(&mut line));
                                x = 0.0;
                                piece_width = 0.0;
                            }
                            piece.push(ch);
                            piece_width += ch_width;
                        }
                        push_segment(
                            &mut line,
                            Segment {
                                text: piece,
                                style: run.style.clone(),
                                width: piece_width,
                            },
                        );
                        x += piece_width;
                    } else {
                        push_segment(
                            &mut line,
                            Segment {
                                text: word.to_string(),
                                style: run.style.clone(),
                                width,
                            },
                        );
                        x += width;
                    }
                }
            }
        }
    }

    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

fn push_segment(line: &mut Vec<Segment>, segment: Segment) {
    if segment.text.is_empty() {
        return;
    }
    if let Some(last) = line.last_mut()
        && last.style == segment.style
    {
        last.text.push_str(&segment.text);
        last.width += segment.width;
        return;
    }
    line.push(segment);
}

enum Token<'t> {
    Word(&'t str),
    Space,
    Newline,
}

fn tokens(text: &str) -> Vec<Token<'_>> {
    let mut out = Vec::new();
    let mut start = None;
    for (idx, ch) in text.char_indices() {
        if ch.is_whitespace() {
            if let Some(word_start) = start.take() {
                out.push(Token::Word(&text[word_start..idx]));
            }
            out.push(if ch == '\n' {
                Token::Newline
            } else {
                Token::Space
            });
        } else if start.is_none() {
            start = Some(idx);
        }
    }
    if let Some(word_start) = start {
        out.push(Token::Word(&text[word_start..]));
    }
    out
}

fn split_chars(line: &str, max_chars: usize) -> Vec<String> {
    let chars: Vec<char> = line.chars().collect();
    if chars.is_empty() {
        return vec![String::new()];
    }
    chars
        .chunks(max_chars)
        .map(|chunk| chunk.iter().collect())
        .collect()
}

fn table_rows<'a>(node: &'a AstNode<'a>) -> Vec<TableRow> {
    node.children()
        .filter_map(|row| {
            let is_header = matches!(row.data().value, NodeValue::TableRow(true));
            let cells = row
                .children()
                .map(|cell| markdown::inline_runs(cell, InlineStyle::default()))
                .collect::<Vec<_>>();
            (!cells.is_empty()).then_some(TableRow { cells, is_header })
        })
        .collect()
}

fn column_widths(rows: &[TableRow], total_width: f32, col_count: usize) -> Vec<f32> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn wrap(text: &str, style: InlineStyle, max_width: f32) -> Vec<Vec<Segment>> {
        wrap_with_runs(
            &[InlineRun {
                text: text.to_string(),
                style,
            }],
            max_width,
        )
    }

    fn wrap_with_runs(runs: &[InlineRun], max_width: f32) -> Vec<Vec<Segment>> {
        // 1pt per char keeps expectations readable
        wrap_runs(runs, max_width, &mut |text, _| text.chars().count() as f32)
    }

    fn line_texts(lines: &[Vec<Segment>]) -> Vec<String> {
        lines
            .iter()
            .map(|line| line.iter().map(|s| s.text.as_str()).collect())
            .collect()
    }

    #[test]
    fn wrap_merges_same_style_words_into_one_segment() {
        let lines = wrap("hello brave world", InlineStyle::default(), 20.0);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].len(), 1);
        assert_eq!(lines[0][0].text, "hello brave world");
        assert_eq!(lines[0][0].width, 17.0);
    }

    #[test]
    fn wrap_keeps_link_phrase_in_one_segment() {
        let link = InlineStyle {
            link: Some("https://example.com".to_string()),
            ..Default::default()
        };
        let runs = [
            InlineRun {
                text: "see ".to_string(),
                style: InlineStyle::default(),
            },
            InlineRun {
                text: "the linked page".to_string(),
                style: link.clone(),
            },
        ];
        let lines = wrap_with_runs(&runs, 40.0);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].len(), 2);
        assert_eq!(lines[0][1].text, "the linked page");
        assert_eq!(lines[0][1].style, link);
    }

    #[test]
    fn wrap_breaks_words_at_line_boundaries() {
        let lines = wrap("aaaa bbbb cc", InlineStyle::default(), 9.0);
        assert_eq!(line_texts(&lines), ["aaaa bbbb", "cc"]);
    }

    #[test]
    fn wrap_breaks_overlong_tokens_at_character_level() {
        let lines = wrap(&"a".repeat(25), InlineStyle::default(), 10.0);
        assert_eq!(
            line_texts(&lines),
            ["a".repeat(10), "a".repeat(10), "a".repeat(5)]
        );
    }

    #[test]
    fn wrap_collapses_whitespace_and_trims_line_edges() {
        let lines = wrap("  a \t  b  ", InlineStyle::default(), 10.0);
        assert_eq!(line_texts(&lines), ["a b"]);
    }

    #[test]
    fn wrap_honors_explicit_line_breaks() {
        let lines = wrap("a\nb", InlineStyle::default(), 10.0);
        assert_eq!(line_texts(&lines), ["a", "b"]);
    }
}
