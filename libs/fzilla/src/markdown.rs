//! Shared comrak parsing helpers: parse options and extraction of styled
//! inline runs from a block node's subtree.

use comrak::{
    Options,
    nodes::{AstNode, NodeValue},
};

pub fn options() -> Options<'static> {
    let mut options = Options::default();
    options.extension.table = true;
    options.extension.strikethrough = true;
    options.extension.autolink = true;
    options.extension.tasklist = true;
    options.parse.relaxed_autolinks = true;
    options
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct InlineStyle {
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub strikethrough: bool,
    pub link: Option<String>,
}

#[derive(Clone)]
pub struct InlineRun {
    pub text: String,
    pub style: InlineStyle,
}

pub fn task_marker(symbol: Option<char>) -> &'static str {
    if symbol.is_some() { "[x] " } else { "[ ] " }
}

/// Flattens the inline content of `node`'s subtree into style-coalesced runs.
/// Line breaks become "\n", soft breaks a space.
pub fn inline_runs<'a>(node: &'a AstNode<'a>, style: InlineStyle) -> Vec<InlineRun> {
    let mut runs = Vec::new();
    collect(node, style, &mut runs);
    runs
}

fn collect<'a>(node: &'a AstNode<'a>, style: InlineStyle, runs: &mut Vec<InlineRun>) {
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
            collect_children(node, nested, runs);
        }
        NodeValue::Emph => {
            let mut nested = style;
            nested.italic = true;
            collect_children(node, nested, runs);
        }
        NodeValue::Strikethrough => {
            let mut nested = style;
            nested.strikethrough = true;
            collect_children(node, nested, runs);
        }
        NodeValue::Link(link) => {
            let mut nested = style;
            nested.link = Some(link.url);
            let before = runs.len();
            collect_children(node, nested.clone(), runs);
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
        _ => collect_children(node, style, runs),
    }
}

fn collect_children<'a>(node: &'a AstNode<'a>, style: InlineStyle, runs: &mut Vec<InlineRun>) {
    for child in node.children() {
        collect(child, style.clone(), runs);
    }
}

fn push_run(runs: &mut Vec<InlineRun>, text: String, style: InlineStyle) {
    if text.is_empty() {
        return;
    }
    if let Some(last) = runs.last_mut()
        && last.style == style
    {
        last.text.push_str(&text);
        return;
    }
    runs.push(InlineRun { text, style });
}

fn plain_text<'a>(node: &'a AstNode<'a>) -> String {
    match node.data().value.clone() {
        NodeValue::Text(text) => text.into_owned(),
        NodeValue::Code(code) => code.literal,
        NodeValue::SoftBreak | NodeValue::LineBreak => " ".to_string(),
        _ => node.children().map(plain_text).collect::<Vec<_>>().join(""),
    }
}
