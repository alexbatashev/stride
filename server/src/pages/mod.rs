pub mod agent;
pub mod archived;
pub mod auth;
pub mod automations;
pub mod files;
pub mod settings;

use std::collections::{HashMap, HashSet};

use crate::api::threads::{
    MessageTemplateData, RunTemplateData, ThreadPageData, ToolCallTemplateData,
};
use crate::components::{
    app_approval_bar::AppApprovalBar,
    app_button::AppButton,
    app_message::AppMessage,
    app_prompt_input::{AppPromptInput, Models},
    app_quiz_bar::AppQuizBar,
    app_run_group::AppRunGroup,
    app_sidebar::{AppSidebar, AppSidebarToggle, SidebarProject, SidebarThread},
    app_tool_call::AppToolCall,
    auth_form::AuthForm,
};

fn html_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

// Adds page-specific host attributes (class, data-*) to a rendered component.
// Argon escapes '>' inside attribute values, so the first '>' closes the host tag.
fn with_attrs(rendered: &str, attrs: &str) -> String {
    rendered.replacen('>', &format!(" {attrs}>"), 1)
}

const NAVIGATE_SCRIPT: &str = r#"<script type="module">
    document.addEventListener('navigate', (e) => {
        window.location.href = e.detail.path === '/login' ? '/auth/login' : e.detail.path;
    });
</script>"#;

// A `<script type="module">` tag for a built page bundle, cache-busted with the
// asset's content hash. Page modules build their page_script() through this.
pub(super) fn module_script(rel: &str) -> String {
    format!(
        r#"<script type="module" src="{}"></script>"#,
        crate::assets::url(rel)
    )
}

pub fn render_page(title: &str, page_script: &str, body_attrs: &str, body: &str) -> String {
    let attrs = if body_attrs.is_empty() {
        String::new()
    } else {
        format!(" {body_attrs}")
    };
    let common_css = crate::assets::url("common.css");
    let components_js = crate::assets::url("components.js");
    let api_js = crate::assets::url("api.js");
    format!(
        r#"<!doctype html>
<html lang="en">
    <head>
        <meta charset="utf-8" />
        <meta name="viewport" content="width=device-width, initial-scale=1" />
        <title>{title}</title>
        <link rel="stylesheet" href="{common_css}" />
        <link rel="modulepreload" href="{components_js}">
        <script type="module" src="{components_js}"></script>
        <script type="module" src="{api_js}"></script>
        {page_script}
    </head>
    <body{attrs}>
        {body}
    </body>
</html>"#,
        title = html_escape(title),
    )
}

pub fn render_auth_page(mode: &str) -> String {
    let title = if mode == "register" {
        "Register"
    } else {
        "Log in"
    };
    let body = format!(
        r#"<style>
    #auth-page {{
        align-items: center;
        box-sizing: border-box;
        justify-content: center;
        padding: 24px;
    }}

    #auth-page .auth-shell {{
        width: 100%;
        max-width: 400px;
    }}
</style>
<div class="auth-shell">{form}</div>
<script type="module">
    document.addEventListener('auth-success', () => {{ window.location.href = '/threads'; }});
</script>"#,
        form = AuthForm::new(mode, "", false).render(),
    );
    render_page(title, "", r#"id="auth-page""#, &body)
}

fn render_sidebar(
    data: &ThreadPageData,
    files_active: bool,
    automations_active: bool,
    settings_active: bool,
    archived_active: bool,
) -> String {
    let projects = data.projects.iter().map(|project| SidebarProject {
        id: project.id.clone(),
        title: html_escape(&project.title),
        threads: project
            .threads
            .iter()
            .map(|thread| SidebarThread {
                id: thread.id.clone(),
                title: html_escape(&thread.title),
            })
            .collect(),
    });
    let ungrouped = data.ungrouped_threads.iter().map(|thread| SidebarThread {
        id: thread.id.clone(),
        title: html_escape(&thread.title),
    });
    let sidebar = AppSidebar::new(
        projects,
        ungrouped,
        &data.thread_id,
        files_active,
        automations_active,
        settings_active,
        archived_active,
    );
    format!("<nav>{sidebar}</nav>")
}

fn render_flat_message(message: &MessageTemplateData) -> String {
    render_message(message, false)
}

fn render_message(message: &MessageTemplateData, suppress_tool_name: bool) -> String {
    let content = if message.message_type == "agent" && message.format == "html" {
        message.content.clone()
    } else {
        html_escape(&message.content)
    };
    // Inside a run group the tool slots sit right below, so the "Called tool X"
    // footer is redundant noise; drop it for group children.
    let tool_name = if suppress_tool_name {
        String::new()
    } else {
        message
            .tool_name
            .as_deref()
            .map(html_escape)
            .unwrap_or_default()
    };
    AppMessage::new(
        &message.id,
        message.seq as f64,
        message.role,
        message.source,
        message.message_type,
        message.format,
        content,
        message
            .thinking
            .as_deref()
            .map(html_escape)
            .unwrap_or_default(),
        tool_name,
    )
    .render()
}

fn render_tool_call(call: &ToolCallTemplateData) -> String {
    AppToolCall::new(
        &call.tool_call_id,
        html_escape(&call.name),
        call.status,
        call.background,
        call.started_at_ms as f64,
        call.finished_at_ms as f64,
        false,
        html_escape(&call.content),
        call.format,
        html_escape(&call.result_text),
    )
    .render()
}

// The seq at which a run's items anchor into the timeline: the smallest seq of
// any message belonging to the run (its triggering user message in practice).
fn run_anchor_seq(run: &RunTemplateData, messages: &[MessageTemplateData]) -> u64 {
    messages
        .iter()
        .filter(|m| m.run_id.as_deref() == Some(&run.id))
        .map(|m| m.seq)
        .min()
        .unwrap_or(u64::MAX)
}

// The message a run treats as its final response: the authoritative
// final_message_id when known, else — while no final is committed — the newest
// agent message provided no tool call was issued at or after it. Mirrors the
// client hydrator so SSR/hydration placement agrees.
fn candidate_final_id<'a>(
    run: &'a RunTemplateData,
    messages: &'a [MessageTemplateData],
) -> Option<&'a str> {
    if let Some(id) = run.final_message_id.as_deref() {
        return Some(id);
    }

    let best = messages
        .iter()
        .filter(|m| m.run_id.as_deref() == Some(&run.id))
        .filter(|m| Some(&m.id) != run.user_message_id.as_ref())
        .filter(|m| m.role == "agent" && m.tool_name.is_none() && m.source != "tool_wakeup")
        .max_by_key(|m| m.seq)?;

    let seq_by_id: HashMap<&str, u64> = messages.iter().map(|m| (m.id.as_str(), m.seq)).collect();
    for call in &run.tool_calls {
        let anchor = call
            .assistant_message_id
            .as_deref()
            .and_then(|id| seq_by_id.get(id).copied());
        if anchor.is_some_and(|anchor| anchor >= best.seq) {
            return None;
        }
    }
    Some(&best.id)
}

// A run group's light-DOM children: intermediate agent notes and tool-call
// slots ordered by (anchor seq, call_seq) — matching the client hydrator.
fn render_run_group(run: &RunTemplateData, messages: &[MessageTemplateData]) -> String {
    let seq_by_id: HashMap<&str, u64> = messages.iter().map(|m| (m.id.as_str(), m.seq)).collect();
    let final_id = candidate_final_id(run, messages);

    enum Item<'a> {
        Message(&'a MessageTemplateData),
        Tool(&'a ToolCallTemplateData),
    }
    let mut items: Vec<(u64, u64, Item)> = Vec::new();
    for message in messages {
        if message.run_id.as_deref() != Some(&run.id) {
            continue;
        }
        if Some(message.id.as_str()) == run.user_message_id.as_deref()
            || Some(message.id.as_str()) == final_id
        {
            continue;
        }
        if message.source == "human" || message.role == "tool" || message.source == "tool_wakeup" {
            continue;
        }
        items.push((message.seq, 0, Item::Message(message)));
    }
    for call in &run.tool_calls {
        let anchor = call
            .assistant_message_id
            .as_deref()
            .and_then(|id| seq_by_id.get(id).copied())
            .unwrap_or(call.started_at_ms as u64);
        items.push((anchor, 1 + call.call_seq as u64, Item::Tool(call)));
    }
    items.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));

    let children = items
        .into_iter()
        .map(|(_, _, item)| match item {
            Item::Message(message) => render_message(message, true),
            Item::Tool(call) => render_tool_call(call),
        })
        .collect::<Vec<_>>()
        .join("\n");

    let group = AppRunGroup::new(
        &run.id,
        run.status,
        run.started_at_ms as f64,
        run.finished_at_ms as f64,
        false,
    )
    .render();
    // Children ride in the host's light DOM, after the shadow template, so the
    // client hydrator reconciles them by key without duplicating nodes.
    group.replacen(
        "</app-run-group>",
        &format!("{children}</app-run-group>"),
        1,
    )
}

// Builds the run-grouped timeline. User (human) messages, legacy messages
// (run_id null), and run final responses render flat; each run gets one group
// slotted at its anchor seq. Runs order by start time / anchor.
fn render_messages(data: &ThreadPageData) -> String {
    if data.thread_id.is_empty() || (data.messages.is_empty() && data.runs.is_empty()) {
        return r#"<div class="empty" data-empty>
                <h2>What are we working on?</h2>
                <p>Start a thread and S.T.R.I.D.E. will keep the context here.</p>
            </div>"#
            .to_string();
    }

    let run_ids: HashSet<&str> = data.runs.iter().map(|r| r.id.as_str()).collect();
    let final_ids: HashSet<&str> = data
        .runs
        .iter()
        .filter_map(|r| candidate_final_id(r, &data.messages))
        .collect();
    let user_trigger_ids: HashSet<&str> = data
        .runs
        .iter()
        .filter_map(|r| r.user_message_id.as_deref())
        .collect();

    enum Entry<'a> {
        Message(&'a MessageTemplateData),
        Group(&'a RunTemplateData),
    }
    let mut entries: Vec<(u64, u8, Entry)> = Vec::new();
    for message in &data.messages {
        match &message.run_id {
            Some(run_id) if run_ids.contains(run_id.as_str()) => {
                if user_trigger_ids.contains(message.id.as_str()) {
                    entries.push((message.seq, 0, Entry::Message(message)));
                } else if final_ids.contains(message.id.as_str()) {
                    entries.push((message.seq, 2, Entry::Message(message)));
                }
            }
            _ => entries.push((message.seq, 0, Entry::Message(message))),
        }
    }
    for run in &data.runs {
        entries.push((run_anchor_seq(run, &data.messages), 1, Entry::Group(run)));
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));

    entries
        .into_iter()
        .map(|(_, _, entry)| match entry {
            Entry::Message(message) => render_flat_message(message),
            Entry::Group(run) => render_run_group(run, &data.messages),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

const THREADS_STYLE: &str = r#"<style>
    #threads-page > main > header {
        border-bottom: 1px solid var(--border);
        box-sizing: border-box;
        display: flex;
        justify-content: flex-end;
    }

    #threads-page > main > header app-sidebar-toggle {
        display: none;
    }

    #threads-page .toolbar-spacer {
        flex: 1;
    }

    #threads-page .files-button {
        min-width: 72px;
    }

    #threads-page .thread-menu-button {
        display: none;
        margin-left: 4px;
    }

    #threads-page .empty {
        align-content: center;
        display: grid;
        justify-items: center;
        min-height: 100%;
        padding-bottom: 96px;
        text-align: center;
    }

    #threads-page .empty h2 {
        color: var(--foreground);
        font-size: clamp(28px, 4vw, 40px);
        font-weight: 700;
        line-height: 1.08;
        margin: 0 0 12px;
    }

    #threads-page .empty p {
        color: var(--muted-foreground);
        font-size: 15px;
        line-height: 1.5;
        margin: 0;
        max-width: 420px;
    }

    #threads-page .error {
        color: var(--destructive);
        font-size: 13px;
        margin: 10px auto 0;
        max-width: 860px;
    }

    #threads-page .error:empty {
        display: none;
    }

    @media (max-width: 767px) {
        #threads-page > main > header app-sidebar-toggle {
            display: inline-flex;
        }

        #threads-page > main > header {
            justify-content: space-between;
        }
    }
</style>"#;

pub fn render_threads_page(data: &ThreadPageData) -> String {
    let sidebar = render_sidebar(data, false, false, false, false);
    let toggle = AppSidebarToggle::new("").render();
    let files_button = with_attrs(
        &AppButton::new().render(),
        r#"variant="ghost" size="sm" class="files-button" data-action="files""#,
    );
    // The slot content goes in the host's light DOM, after the shadow template.
    let files_button = files_button.replacen("</app-button>", "Files</app-button>", 1);
    let menu_button = with_attrs(
        &AppButton::new().render(),
        r#"variant="ghost" size="icon-sm" class="thread-menu-button" title="Thread actions" aria-label="Thread actions" data-action="thread-menu""#,
    );
    let menu_button = menu_button.replacen("</app-button>", "⋯</app-button>", 1);
    let messages = render_messages(data);
    let placeholder = if data.thread_id.is_empty() {
        "Ask S.T.R.I.D.E. anything"
    } else {
        "Message S.T.R.I.D.E."
    };
    let prompt = with_attrs(
        &AppPromptInput::new(false, data.running, placeholder, Vec::<Models>::new(), "").render(),
        r#"style="margin: auto" data-prompt"#,
    );
    let approval = with_attrs(
        &AppApprovalBar::new("").render(),
        r#"style="margin: auto" data-approval hidden"#,
    );
    let quiz = with_attrs(
        &AppQuizBar::new("", Vec::<String>::new()).render(),
        r#"style="margin: auto" data-quiz hidden"#,
    );
    let current_title = html_escape(&data.current_title);
    let thread_id = html_escape(&data.thread_id);

    let body = format!(
        r#"{THREADS_STYLE}
{sidebar}
<main>
    <header>
        {toggle}
        <span class="toolbar-spacer"></span>
        {files_button}
        {menu_button}
        <span data-current-title hidden>{current_title}</span>
    </header>
    <section class="content">
        <div class="wrapper" data-messages>
            {messages}
        </div>
    </section>
    {prompt}
    {approval}
    {quiz}
    <div class="error" data-error></div>
</main>
<app-file-manager data-file-manager data-thread-id="{thread_id}"></app-file-manager>
{NAVIGATE_SCRIPT}"#,
    );

    let body_attrs = format!(
        r#"id="threads-page" data-thread-id="{thread_id}" data-running="{running}""#,
        running = data.running,
    );
    render_page("S.T.R.I.D.E.", &agent::page_script(), &body_attrs, &body)
}

const FILES_STYLE: &str = r#"<style>
    #files-page > main {
        display: flex;
        flex-direction: column;
        min-height: 0;
        min-width: 0;
    }

    #files-page app-file-browser {
        flex: 1;
        min-height: 0;
    }

    #files-page .mobile-bar {
        display: none;
    }

    @media (max-width: 767px) {
        #files-page .mobile-bar {
            border-bottom: 1px solid var(--border);
            display: flex;
            padding: 8px 12px;
        }
    }
</style>"#;

pub fn render_files_page(data: &ThreadPageData) -> String {
    let sidebar = render_sidebar(data, true, false, false, false);
    let toggle = AppSidebarToggle::new("").render();
    let body = format!(
        r#"{FILES_STYLE}
{sidebar}
<main>
    <div class="mobile-bar">{toggle}</div>
    <app-file-browser></app-file-browser>
</main>
{NAVIGATE_SCRIPT}"#,
    );
    render_page(
        "Files - S.T.R.I.D.E.",
        &files::page_script(),
        r#"id="files-page""#,
        &body,
    )
}

const AUTOMATIONS_STYLE: &str = r#"<style>
    #automations-page > main {
        display: flex;
        flex-direction: column;
        min-height: 0;
        min-width: 0;
    }

    #automations-page app-automations {
        flex: 1;
        min-height: 0;
    }

    #automations-page .mobile-bar {
        display: none;
    }

    @media (max-width: 767px) {
        #automations-page .mobile-bar {
            border-bottom: 1px solid var(--border);
            display: flex;
            padding: 8px 12px;
        }
    }
</style>"#;

pub fn render_automations_page(data: &ThreadPageData) -> String {
    let sidebar = render_sidebar(data, false, true, false, false);
    let toggle = AppSidebarToggle::new("").render();
    let body = format!(
        r#"{AUTOMATIONS_STYLE}
{sidebar}
<main>
    <div class="mobile-bar">{toggle}</div>
    <app-automations></app-automations>
</main>
{NAVIGATE_SCRIPT}"#,
    );
    render_page(
        "Automations - S.T.R.I.D.E.",
        &automations::page_script(),
        r#"id="automations-page""#,
        &body,
    )
}

pub fn render_archived_page(data: &ThreadPageData) -> String {
    let sidebar = render_sidebar(data, false, false, false, true);
    let toggle = AppSidebarToggle::new("").render();
    let body = format!(
        r#"<style>
    #archived-page > main {{
        display: flex;
        flex-direction: column;
        min-height: 0;
        min-width: 0;
    }}

    #archived-page app-archived-threads {{
        flex: 1;
        min-height: 0;
    }}

    #archived-page .mobile-bar {{
        display: none;
    }}

    @media (max-width: 767px) {{
        #archived-page .mobile-bar {{
            border-bottom: 1px solid var(--border);
            display: flex;
            padding: 8px 12px;
        }}
    }}
</style>
{sidebar}
<main>
    <div class="mobile-bar">{toggle}</div>
    <app-archived-threads></app-archived-threads>
</main>
{NAVIGATE_SCRIPT}"#,
    );

    render_page(
        "Archived - S.T.R.I.D.E.",
        &archived::page_script(),
        r#"id="archived-page""#,
        &body,
    )
}

pub fn render_settings_page(data: &ThreadPageData) -> String {
    let sidebar = render_sidebar(data, false, false, true, false);
    let toggle = AppSidebarToggle::new("").render();

    let body = format!(
        r#"<style>
    #settings-page > main {{
        display: flex;
        flex-direction: column;
        min-height: 0;
        min-width: 0;
    }}

    #settings-page app-settings {{
        flex: 1;
        min-height: 0;
    }}

    #settings-page .mobile-bar {{
        display: none;
    }}

    @media (max-width: 767px) {{
        #settings-page .mobile-bar {{
            border-bottom: 1px solid var(--border);
            display: flex;
            padding: 8px 12px;
        }}
    }}
</style>
{sidebar}
<main>
    <div class="mobile-bar">{toggle}</div>
    <app-settings></app-settings>
</main>
{NAVIGATE_SCRIPT}"#,
    );

    render_page(
        "Settings - S.T.R.I.D.E.",
        &settings::page_script(),
        r#"id="settings-page""#,
        &body,
    )
}

#[cfg(test)]
mod tests {
    use crate::api::threads::{
        MessageTemplateData, ProjectTemplateData, RunTemplateData, ThreadPageData,
        ThreadTemplateData, ToolCallTemplateData,
    };

    fn sample_data() -> ThreadPageData {
        ThreadPageData {
            thread_id: "thread-1".to_string(),
            current_title: "Current thread".to_string(),
            running: true,
            projects: vec![ProjectTemplateData {
                id: "project-1".to_string(),
                title: "My <Project>".to_string(),
                threads: vec![ThreadTemplateData {
                    id: "thread-1".to_string(),
                    title: "Current thread".to_string(),
                    project_id: Some("project-1".to_string()),
                    active: true,
                }],
            }],
            ungrouped_threads: vec![ThreadTemplateData {
                id: "thread-2".to_string(),
                title: "Loose thread".to_string(),
                project_id: None,
                active: false,
            }],
            messages: vec![
                MessageTemplateData {
                    id: "message-1".to_string(),
                    seq: 1,
                    role: "tool",
                    source: "system",
                    format: "markdown",
                    message_type: "tool_output",
                    tool_name: Some("Tool output".to_string()),
                    content: "done".to_string(),
                    thinking: None,
                    has_thinking: false,
                    run_id: None,
                    tool_call_id: Some("legacy-tool".to_string()),
                },
                MessageTemplateData {
                    id: "message-2".to_string(),
                    seq: 2,
                    role: "agent",
                    source: "system",
                    format: "html",
                    message_type: "agent",
                    tool_name: None,
                    content: "hello & <world>".to_string(),
                    thinking: None,
                    has_thinking: false,
                    run_id: None,
                    tool_call_id: None,
                },
                MessageTemplateData {
                    id: "user-run".to_string(),
                    seq: 3,
                    role: "user",
                    source: "human",
                    format: "markdown",
                    message_type: "user",
                    tool_name: None,
                    content: "run something".to_string(),
                    thinking: None,
                    has_thinking: false,
                    run_id: Some("run-1".to_string()),
                    tool_call_id: None,
                },
                MessageTemplateData {
                    id: "agent-final".to_string(),
                    seq: 5,
                    role: "agent",
                    source: "system",
                    format: "markdown",
                    message_type: "agent",
                    tool_name: None,
                    content: "all done".to_string(),
                    thinking: None,
                    has_thinking: false,
                    run_id: Some("run-1".to_string()),
                    tool_call_id: None,
                },
            ],
            runs: vec![RunTemplateData {
                id: "run-1".to_string(),
                status: "finished",
                started_at_ms: 1_000_000,
                finished_at_ms: 1_005_000,
                user_message_id: Some("user-run".to_string()),
                final_message_id: Some("agent-final".to_string()),
                tool_calls: vec![ToolCallTemplateData {
                    tool_call_id: "call-1".to_string(),
                    name: "Shell".to_string(),
                    status: "finished",
                    background: false,
                    started_at_ms: 1_001_000,
                    finished_at_ms: 1_002_000,
                    call_seq: 0,
                    format: "plaintext",
                    assistant_message_id: Some("user-run".to_string()),
                    content: "stdout".to_string(),
                    result_text: String::new(),
                }],
            }],
        }
    }

    #[test]
    fn threads_page_renders_shell_components_and_messages() {
        let html = super::render_threads_page(&sample_data());

        assert!(
            html.contains(
                r#"<body id="threads-page" data-thread-id="thread-1" data-running="true">"#
            )
        );
        // Server-side shadow DOM for the chrome that should paint before JS.
        assert!(html.contains("<nav><app-sidebar"));
        assert!(html.contains(r#"<template shadowrootmode="open">"#));
        assert!(html.contains("My &lt;Project&gt;"));
        assert!(html.contains("Loose thread"));
        // Messages arrive as hydrated app-message components. Tool output is
        // folded into a spoiler, so its content rides in the hydration
        // attribute; agent text paints inside the shadow DOM markdown view.
        assert!(html.contains(r#"data-message-id="message-1""#));
        assert!(html.contains(r#"data-kind="tool_output""#));
        assert!(html.contains(r#"data-message-id="message-2""#));
        assert!(html.contains("hello &amp; &lt;world&gt;"));
        // The run renders as a group wrapping its tool-call slot, with the
        // triggering user message and final response flat outside it.
        assert!(html.contains(r#"<app-run-group data-run-id="run-1""#));
        assert!(html.contains(r#"data-tool-call-id="call-1""#));
        assert!(html.contains(r#"data-message-id="user-run""#));
        assert!(html.contains(r#"data-message-id="agent-final""#));
        // The tool-call slot nests inside its run group, not at the top level.
        let group_start = html.find(r#"<app-run-group data-run-id="run-1""#).unwrap();
        let group_end = html[group_start..].find("</app-run-group>").unwrap() + group_start;
        let call_at = html.find(r#"data-tool-call-id="call-1""#).unwrap();
        assert!(call_at > group_start && call_at < group_end);
        // Composer state mirrors the running flag.
        assert!(html.contains(r#"data-running="true""#));
        assert!(html.contains(r#"data-prompt"#));
        assert!(html.contains(r#"data-approval hidden"#));
        assert!(html.contains(r#"data-quiz hidden"#));
        assert!(html.contains(r#"<app-file-manager data-file-manager data-thread-id="thread-1">"#));
        assert!(html.contains("/static/pages/threads-page.js"));
        assert!(!html.contains("lit.js"));
    }

    #[test]
    fn threads_page_without_thread_shows_empty_state() {
        let mut data = sample_data();
        data.thread_id = String::new();
        data.messages = Vec::new();
        let html = super::render_threads_page(&data);

        assert!(html.contains("What are we working on?"));
        assert!(html.contains("Ask S.T.R.I.D.E. anything"));
    }

    #[test]
    fn files_page_renders_browser_and_active_nav() {
        let html = super::render_files_page(&sample_data());

        assert!(html.contains(r#"<body id="files-page">"#));
        assert!(html.contains(r#"<app-file-browser></app-file-browser>"#));
        // The Files nav item is marked active inside the SSR shadow DOM.
        assert!(html.contains(r#"href="/files" aria-current="page""#));
        assert!(html.contains("/static/pages/files-page.js"));
    }

    #[test]
    fn automations_page_renders_list_shell() {
        let html = super::render_automations_page(&sample_data());

        assert!(html.contains(r#"<body id="automations-page">"#));
        assert!(html.contains("<app-automations></app-automations>"));
        assert!(html.contains(r#"href="/automations" aria-current="page""#));
        assert!(html.contains("/static/pages/automations-page.js"));
    }

    #[test]
    fn archived_page_renders_list_shell() {
        let html = super::render_archived_page(&sample_data());

        assert!(html.contains(r#"<body id="archived-page">"#));
        assert!(html.contains("<app-archived-threads></app-archived-threads>"));
        assert!(html.contains(r#"href="/archived" aria-current="page""#));
        assert!(html.contains("/static/pages/archived-page.js"));
    }

    #[test]
    fn settings_page_renders_app_settings_shell() {
        let html = super::render_settings_page(&sample_data());

        assert!(html.contains(r#"<body id="settings-page">"#));
        // The settings UI is a single client-hydrated Argon component.
        assert!(html.contains("<app-settings></app-settings>"));
        assert!(html.contains(r#"href="/settings" aria-current="page""#));
        assert!(html.contains("/static/pages/settings-page.js"));
    }

    #[test]
    fn auth_page_renders_form_for_mode() {
        let html = super::render_auth_page("register");
        assert!(html.contains(r#"data-mode="register""#));
        assert!(html.contains("Create account"));
        // Mode switch is a plain link to the other auth route, not a JS event.
        assert!(html.contains(r#"href="/auth/login""#));
    }
}
