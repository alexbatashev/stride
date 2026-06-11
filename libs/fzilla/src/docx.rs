//! Markdown to DOCX rendering via office_oxide's document IR.
//!
//! Nested markdown lists are flattened into sibling top-level IR lists:
//! office_oxide hardcodes bullet numbering for lists nested inside list
//! items, so each depth becomes its own list with an explicit start number
//! (to continue the parent's numbering) and paragraph indentation.

use std::io::Cursor;

use anyhow::{Context, Result};
use comrak::{
    nodes::{AstNode, ListType, NodeList, NodeValue},
    parse_document, Arena,
};
use office_oxide::{create::create_from_ir_to_writer, ir, DocumentFormat};

use crate::markdown::{self, task_marker, InlineStyle};

const LIST_INDENT_TWIPS: i32 = 360;

pub fn render(markdown: &str) -> Result<Vec<u8>> {
    let ir = document_ir(markdown);
    let mut writer = Cursor::new(Vec::new());
    create_from_ir_to_writer(&ir, DocumentFormat::Docx, &mut writer).context("write DOCX")?;
    Ok(writer.into_inner())
}

fn document_ir(markdown: &str) -> ir::DocumentIR {
    let arena = Arena::new();
    let root = parse_document(&arena, markdown, &crate::markdown::options());
    let mut elements = Vec::new();
    for child in root.children() {
        collect_blocks(child, 0, &mut elements);
    }

    ir::DocumentIR {
        metadata: ir::Metadata {
            format: DocumentFormat::Docx,
            ..Default::default()
        },
        sections: vec![ir::Section {
            elements,
            ..Default::default()
        }],
    }
}

fn collect_blocks<'a>(node: &'a AstNode<'a>, indent_twips: i32, elements: &mut Vec<ir::Element>) {
    match node.data().value.clone() {
        NodeValue::Paragraph => {
            let content = inline_content(node);
            if !content.is_empty() {
                elements.push(ir::Element::Paragraph(ir::Paragraph {
                    content,
                    indent_left_twips: (indent_twips > 0).then_some(indent_twips),
                    ..Default::default()
                }));
            }
        }
        NodeValue::Heading(heading) => {
            let content = inline_content(node);
            if !content.is_empty() {
                elements.push(ir::Element::Heading(ir::Heading {
                    level: heading.level.clamp(1, 6),
                    content,
                    ..Default::default()
                }));
            }
        }
        NodeValue::List(list) => flatten_list(node, &list, indent_twips, 0, elements),
        NodeValue::BlockQuote | NodeValue::MultilineBlockQuote(_) => {
            for child in node.children() {
                collect_blocks(child, indent_twips + LIST_INDENT_TWIPS, elements);
            }
        }
        NodeValue::CodeBlock(code) => {
            elements.push(ir::Element::CodeBlock(ir::CodeBlock {
                language: (!code.info.trim().is_empty()).then(|| code.info.trim().to_string()),
                content: code.literal,
            }));
        }
        NodeValue::ThematicBreak => elements.push(ir::Element::ThematicBreak),
        NodeValue::Table(_) => elements.push(ir::Element::Table(table(node))),
        _ => {
            for child in node.children() {
                collect_blocks(child, indent_twips, elements);
            }
        }
    }
}

fn flatten_list<'a>(
    node: &'a AstNode<'a>,
    list: &NodeList,
    indent_twips: i32,
    depth: u8,
    elements: &mut Vec<ir::Element>,
) {
    let ordered = list.list_type == ListType::Ordered;
    let item_indent = indent_twips + LIST_INDENT_TWIPS * depth as i32;
    let mut group_start = list.start.max(1) as u32;
    let mut items: Vec<ir::ListItem> = Vec::new();

    for (next, item) in (list.start.max(1) as u32..).zip(node.children()) {
        let marker = match &item.data().value {
            NodeValue::TaskItem(task) => Some(task_marker(task.symbol)),
            _ => None,
        };
        let mut content = Vec::new();
        let mut emitted = false;
        for child in item.children() {
            match child.data().value.clone() {
                NodeValue::List(nested) => {
                    if !emitted {
                        if items.is_empty() {
                            group_start = next;
                        }
                        items.push(list_item(std::mem::take(&mut content), marker));
                        emitted = true;
                    }
                    flush_list(&mut items, ordered, group_start, elements);
                    flatten_list(child, &nested, indent_twips, depth + 1, elements);
                }
                // continuation content after a nested list cannot rejoin the
                // numbering; emit it as indented paragraphs
                _ if emitted => collect_blocks(child, item_indent + LIST_INDENT_TWIPS, elements),
                _ => collect_blocks(child, item_indent, &mut content),
            }
        }
        if !emitted {
            if items.is_empty() {
                group_start = next;
            }
            items.push(list_item(content, marker));
        }
    }
    flush_list(&mut items, ordered, group_start, elements);
}

fn flush_list(
    items: &mut Vec<ir::ListItem>,
    ordered: bool,
    start: u32,
    elements: &mut Vec<ir::Element>,
) {
    if items.is_empty() {
        return;
    }
    elements.push(ir::Element::List(ir::List {
        ordered,
        items: std::mem::take(items),
        start_number: (ordered && start > 1).then_some(start),
        ..Default::default()
    }));
}

fn list_item(mut content: Vec<ir::Element>, marker: Option<&str>) -> ir::ListItem {
    if let Some(marker) = marker {
        let span = ir::InlineContent::Text(ir::TextSpan::plain(marker));
        match content.first_mut() {
            Some(ir::Element::Paragraph(paragraph)) => paragraph.content.insert(0, span),
            _ => content.insert(
                0,
                ir::Element::Paragraph(ir::Paragraph {
                    content: vec![span],
                    ..Default::default()
                }),
            ),
        }
    }
    ir::ListItem {
        content,
        ..Default::default()
    }
}

fn table<'a>(node: &'a AstNode<'a>) -> ir::Table {
    ir::Table {
        rows: node
            .children()
            .map(|row| {
                let is_header = matches!(row.data().value, NodeValue::TableRow(true));
                ir::TableRow {
                    cells: row
                        .children()
                        .map(|cell| ir::TableCell {
                            content: ir::inline_to_element_block(inline_content(cell)),
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

fn inline_content<'a>(node: &'a AstNode<'a>) -> Vec<ir::InlineContent> {
    let mut content = Vec::new();
    for run in markdown::inline_runs(node, InlineStyle::default()) {
        for (idx, text) in run.text.split('\n').enumerate() {
            if idx > 0 {
                content.push(ir::InlineContent::LineBreak);
            }
            if !text.is_empty() {
                content.push(ir::InlineContent::Text(text_span(text, &run.style)));
            }
        }
    }
    content
}

fn text_span(text: &str, style: &InlineStyle) -> ir::TextSpan {
    ir::TextSpan {
        text: text.to_string(),
        bold: style.bold,
        italic: style.italic,
        strikethrough: style.strikethrough,
        hyperlink: style.link.clone(),
        font_name: style.code.then(|| "Courier New".to_string()),
        highlight: style.code.then_some([242, 242, 238]),
        color: style.link.as_ref().map(|_| [5, 61, 140]),
        underline: style.link.as_ref().map(|_| ir::UnderlineStyle::Single),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lists(elements: &[ir::Element]) -> Vec<&ir::List> {
        elements
            .iter()
            .filter_map(|element| match element {
                ir::Element::List(list) => Some(list),
                _ => None,
            })
            .collect()
    }

    fn item_text(item: &ir::ListItem) -> String {
        item.content
            .iter()
            .filter_map(|element| match element {
                ir::Element::Paragraph(p) => Some(
                    p.content
                        .iter()
                        .filter_map(|content| match content {
                            ir::InlineContent::Text(span) => Some(span.text.as_str()),
                            _ => None,
                        })
                        .collect::<String>(),
                ),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn nested_lists_flatten_with_continued_numbering() {
        let ir = document_ir("1. one\n   - sub one\n   - sub two\n2. two\n");
        let lists = lists(&ir.sections[0].elements);

        assert_eq!(lists.len(), 3);

        assert!(lists[0].ordered);
        assert_eq!(lists[0].start_number, None);
        assert_eq!(item_text(&lists[0].items[0]), "one");

        assert!(!lists[1].ordered);
        assert_eq!(lists[1].items.len(), 2);
        assert_eq!(item_text(&lists[1].items[0]), "sub one");

        assert!(lists[2].ordered);
        assert_eq!(lists[2].start_number, Some(2));
        assert_eq!(item_text(&lists[2].items[0]), "two");
    }

    #[test]
    fn nested_list_items_are_indented() {
        let ir = document_ir("- top\n  - sub\n");
        let lists = lists(&ir.sections[0].elements);

        let top = match &lists[0].items[0].content[0] {
            ir::Element::Paragraph(p) => p,
            other => panic!("expected paragraph, got {other:?}"),
        };
        assert_eq!(top.indent_left_twips, None);

        let sub = match &lists[1].items[0].content[0] {
            ir::Element::Paragraph(p) => p,
            other => panic!("expected paragraph, got {other:?}"),
        };
        assert_eq!(sub.indent_left_twips, Some(LIST_INDENT_TWIPS));
    }

    #[test]
    fn ordered_list_start_is_preserved() {
        let ir = document_ir("5. five\n6. six\n");
        let lists = lists(&ir.sections[0].elements);

        assert_eq!(lists[0].start_number, Some(5));
        assert_eq!(lists[0].items.len(), 2);
    }

    #[test]
    fn task_items_keep_their_checkbox_marker() {
        let ir = document_ir("- [x] done\n- [ ] todo\n");
        let lists = lists(&ir.sections[0].elements);

        assert_eq!(item_text(&lists[0].items[0]), "[x] done");
        assert_eq!(item_text(&lists[0].items[1]), "[ ] todo");
    }
}
