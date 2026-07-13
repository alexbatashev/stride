pub mod agent;
pub mod archived;
pub mod auth;
pub mod automations;
pub mod files;
pub mod settings;

use crate::api::threads::{MessageTemplateData, ThreadPageData};
use crate::components::{
    auth_form::AuthForm,
    shell_page_view::{
        RenderStores as ShellRenderStores, ShellPageData, ShellPageView, ShellPageViewServer,
    },
    threads_page_view::{DocumentOpts as ArgonDocumentOpts, ThreadPageData as ArgonThreadPageData},
    timeline::{ChatTurn, TimelineItem, TimelineMessage, WorkSegment},
    ui::Stores as UiStores,
};

fn argon_thread_page_data(data: ThreadPageData) -> ArgonThreadPageData {
    let running = data.running;
    let selected_model = data
        .models
        .iter()
        .find(|model| model.key == data.selected_model)
        .or_else(|| data.models.first());
    let selected_model_label = selected_model
        .map(|model| model.display_name.clone())
        .unwrap_or_else(|| "Choose model".to_string());
    let selected_model_reasoning_effort = selected_model
        .and_then(|model| model.reasoning_effort.clone())
        .unwrap_or_default();
    let models = data
        .models
        .into_iter()
        .map(|model| crate::components::model_option::ModelOption {
            value: model.key,
            label: model.display_name,
            description: model.description,
            vision: model.vision,
        })
        .collect();
    ArgonThreadPageData {
        thread_id: data.thread_id,
        current_title: data.current_title,
        selected_model: data.selected_model,
        models,
        selected_model_label,
        selected_model_reasoning_effort,
        running,
        projects: data
            .projects
            .into_iter()
            .map(|project| crate::components::app_sidebar::SidebarProject {
                id: project.id,
                title: project.title,
                threads: project
                    .threads
                    .into_iter()
                    .map(|thread| crate::components::app_sidebar::SidebarThread {
                        id: thread.id,
                        title: thread.title,
                    })
                    .collect(),
            })
            .collect(),
        threads: data
            .ungrouped_threads
            .into_iter()
            .map(|thread| crate::components::app_sidebar::SidebarThread {
                id: thread.id,
                title: thread.title,
            })
            .collect(),
        turns: chat_turns(
            crate::components::timeline::build_timeline(&chat_timeline(data.messages)),
            running,
        ),
    }
}

fn empty_timeline_item(id: String) -> TimelineItem {
    TimelineItem {
        id,
        seq: 0.0,
        created_at: 0.0,
        role: "agent".to_string(),
        kind: "agent".to_string(),
        format: "markdown".to_string(),
        text: String::new(),
        thinking: String::new(),
        tool_name: String::new(),
        tool_detail: String::new(),
        status: "finished".to_string(),
        is_error: false,
        pending: false,
        subagent_key: None,
    }
}

fn work_label(started_at: f64, finished_at: f64) -> String {
    if started_at <= 0.0 || finished_at <= started_at {
        return "Worked".to_string();
    }
    let seconds = ((finished_at - started_at) / 1000.0).round().max(1.0) as u64;
    format!("Worked for {seconds}s")
}

fn chat_turn(items: &[TimelineItem], running: bool, index: usize) -> ChatTurn {
    let user_index = items.iter().position(|item| item.kind == "user");
    let answer_index = items
        .iter()
        .rposition(|item| item.kind == "agent" && !item.text.is_empty());
    let fallback_id = format!("turn-{index}");
    let user = user_index
        .map(|item_index| items[item_index].clone())
        .unwrap_or_else(|| empty_timeline_item(format!("{fallback_id}-user")));
    let answer = answer_index
        .map(|item_index| items[item_index].clone())
        .unwrap_or_else(|| empty_timeline_item(format!("{fallback_id}-answer")));
    let mut segments = Vec::new();
    let mut commentary = String::new();
    let mut tools = Vec::new();
    let mut segment_index = 0;

    for (item_index, item) in items.iter().enumerate() {
        if item.kind == "tool_activity" || item.kind == "tool_output" {
            tools.push(item.clone());
            continue;
        }
        if item.kind != "agent" {
            continue;
        }
        let mut next_commentary = item.thinking.clone();
        if Some(item_index) != answer_index && !item.text.is_empty() {
            if !next_commentary.is_empty() {
                next_commentary.push_str("\n\n");
            }
            next_commentary.push_str(&item.text);
        }
        if next_commentary.is_empty() {
            continue;
        }
        if !commentary.is_empty() || !tools.is_empty() {
            segments.push(WorkSegment {
                id: format!("{fallback_id}-work-{segment_index}"),
                commentary,
                tools,
            });
            segment_index += 1;
        }
        commentary = next_commentary;
        tools = Vec::new();
    }

    if !commentary.is_empty() || !tools.is_empty() {
        segments.push(WorkSegment {
            id: format!("{fallback_id}-work-{segment_index}"),
            commentary,
            tools,
        });
    }

    let first = items.first().unwrap_or(&user);
    let last = items.last().unwrap_or(&answer);
    let started_at = if user.created_at > 0.0 {
        user.created_at
    } else {
        first.created_at
    };
    let finished_at = if answer.created_at > 0.0 {
        answer.created_at
    } else {
        last.created_at
    };
    ChatTurn {
        id: user_index.map_or(fallback_id, |_| user.id.clone()),
        has_user: user_index.is_some(),
        user,
        has_work: running || !segments.is_empty(),
        segments,
        has_answer: answer_index.is_some(),
        answer,
        running,
        started_at,
        work_label: work_label(started_at, finished_at),
    }
}

fn chat_turns(messages: Vec<TimelineItem>, running: bool) -> Vec<ChatTurn> {
    let mut turns = Vec::new();
    let mut start = 0;
    for index in 0..messages.len() {
        if messages[index].kind != "user" || index == start {
            continue;
        }
        turns.push(chat_turn(&messages[start..index], false, turns.len()));
        start = index;
    }
    if start < messages.len() {
        turns.push(chat_turn(&messages[start..], running, turns.len()));
    }
    turns
}

fn chat_timeline(messages: Vec<MessageTemplateData>) -> Vec<TimelineMessage> {
    use std::collections::{HashMap, HashSet};

    let outputs: HashMap<_, _> = messages
        .iter()
        .filter_map(|message| {
            message.tool_call_id.as_ref().map(|id| {
                (
                    id.clone(),
                    (message.id.clone(), message.format, message.content.clone()),
                )
            })
        })
        .collect();
    let mut consumed = HashSet::new();
    let mut timeline = Vec::new();

    for message in messages {
        if !message.tool_calls.is_empty() {
            if !message.content.is_empty() || message.thinking.is_some() {
                timeline.push(content_timeline_message(&message, "agent"));
            }
            for call in message.tool_calls {
                let output = outputs.get(&call.id);
                if let Some((id, _, _)) = output {
                    consumed.insert(id.clone());
                }
                if is_subagent_tool(&call.name) {
                    continue;
                }
                timeline.push(tool_timeline_message(
                    format!("tool:{}", call.id),
                    message.seq,
                    message.created_at,
                    output.map_or("markdown", |(_, format, _)| *format),
                    output.map_or_else(String::new, |(_, _, content)| content.clone()),
                    &tool_activity_label(&call.name),
                    &summarize_tool_arguments(&call.arguments),
                ));
            }
            continue;
        }
        if consumed.contains(&message.id) {
            continue;
        }
        let kind = if message.role == "tool" {
            "tool_activity"
        } else {
            message.message_type
        };
        timeline.push(if kind == "tool_activity" {
            tool_timeline_message(
                message.id,
                message.seq,
                message.created_at,
                message.format,
                message.content,
                message.tool_name.as_deref().unwrap_or_default(),
                "",
            )
        } else {
            content_timeline_message(&message, kind)
        });
    }
    timeline
}

fn content_timeline_message(message: &MessageTemplateData, message_type: &str) -> TimelineMessage {
    let content = if message.format == "html" {
        stride_agent::HtmlFormattingSanitizer::sanitize_complete(&message.content)
    } else {
        message.content.clone()
    };
    TimelineMessage {
        id: message.id.clone(),
        seq: message.seq as f64,
        created_at: message.created_at as f64,
        role: message.role.to_string(),
        message_type: message_type.to_string(),
        format: message.format.to_string(),
        content,
        thinking: message.thinking.clone().unwrap_or_default(),
        tool_name: String::new(),
        tool_detail: String::new(),
        pending: false,
        status: "finished".to_string(),
        is_error: false,
        subagent_key: None,
    }
}

fn tool_timeline_message(
    id: String,
    seq: u64,
    created_at: i64,
    format: &str,
    content: String,
    tool_name: &str,
    tool_detail: &str,
) -> TimelineMessage {
    TimelineMessage {
        id,
        seq: seq as f64,
        created_at: created_at as f64,
        role: "tool".to_string(),
        message_type: "tool_activity".to_string(),
        format: format.to_string(),
        content,
        thinking: String::new(),
        tool_name: tool_name.to_string(),
        tool_detail: tool_detail.to_string(),
        pending: false,
        status: "finished".to_string(),
        is_error: false,
        subagent_key: None,
    }
}

fn is_subagent_tool(name: &str) -> bool {
    let normalized = name.to_lowercase().replace('-', "_");
    normalized.contains("subagent") || normalized.contains("spawn_agent")
}

fn tool_activity_label(name: &str) -> String {
    let normalized = name.to_lowercase().replace('-', "_");
    if normalized.contains("command") || normalized.contains("shell") || normalized.contains("exec")
    {
        return "Ran command".to_string();
    }
    if normalized.contains("read_file") || normalized.ends_with("read") {
        return "Read file".to_string();
    }
    if normalized.contains("apply_patch")
        || normalized.contains("write")
        || normalized.contains("edit")
    {
        return "Changed files".to_string();
    }
    if normalized.contains("search") || normalized.contains("find") || normalized.ends_with("rg") {
        return "Searched files".to_string();
    }
    name.replace('_', " ")
}

fn summarize_tool_arguments(arguments: &str) -> String {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(arguments) else {
        return String::new();
    };
    ["path", "command", "query", "url"]
        .into_iter()
        .find_map(|key| value.get(key).and_then(serde_json::Value::as_str))
        .unwrap_or_default()
        .to_string()
}

fn argon_document_opts(title: &str, store_payload: &str) -> ArgonDocumentOpts {
    let common_css = crate::assets::url("common.css");
    let components_js = crate::assets::url("components.js");
    let api_js = crate::assets::url("api.js");
    let store_script = format!(
        r#"<script type="application/json" data-argon-stores>{}</script>"#,
        json_script_escape(store_payload)
    );
    ArgonDocumentOpts {
        title: title.to_string(),
        head: format!(
            r#"<meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><link rel="stylesheet" href="{common_css}"><link rel="modulepreload" href="{components_js}">{store_script}"#
        ),
        assets: format!(
            r#"<script type="module" src="{components_js}"></script><script type="module" src="{api_js}"></script>"#
        ),
    }
}

fn combine_store_snapshots(snapshots: &[String]) -> String {
    let entries = snapshots
        .iter()
        .filter_map(|snapshot| snapshot.strip_prefix('{')?.strip_suffix('}'))
        .filter(|snapshot| !snapshot.is_empty())
        .collect::<Vec<_>>()
        .join(",");
    format!("{{{entries}}}")
}

struct ShellPageServer {
    state: std::sync::Arc<crate::ServerState>,
    headers: axum::http::HeaderMap,
}

impl ShellPageViewServer for ShellPageServer {
    type Error = crate::api::threads::ThreadApiError;

    async fn load_shell_page(&self, _page: &str) -> Result<ShellPageData, Self::Error> {
        let data = crate::api::threads::thread_page_data(&self.state, &self.headers, None).await?;
        Ok(ShellPageData {
            projects: data
                .projects
                .into_iter()
                .map(|project| crate::components::app_sidebar::SidebarProject {
                    id: project.id,
                    title: project.title,
                    threads: project
                        .threads
                        .into_iter()
                        .map(|thread| crate::components::app_sidebar::SidebarThread {
                            id: thread.id,
                            title: thread.title,
                        })
                        .collect(),
                })
                .collect(),
            threads: data
                .ungrouped_threads
                .into_iter()
                .map(|thread| crate::components::app_sidebar::SidebarThread {
                    id: thread.id,
                    title: thread.title,
                })
                .collect(),
        })
    }
}

async fn render_shell_page(
    state: std::sync::Arc<crate::ServerState>,
    headers: axum::http::HeaderMap,
    page_name: &str,
    title: &str,
) -> Result<String, crate::api::threads::ThreadApiError> {
    let mut ui_stores = UiStores::default();
    ui_stores.sidebar.active_page = page_name.to_string();
    let stores = ShellRenderStores { ui: &ui_stores };
    let server = ShellPageServer { state, headers };
    let page = ShellPageView::new(page_name).attr("id", format!("{page_name}-page"));
    let opts = argon_document_opts(title, &ui_stores.snapshot_json());
    let opts = crate::components::shell_page_view::DocumentOpts {
        title: opts.title,
        head: opts.head,
        assets: opts.assets,
    };
    page.render_document(&server, &stores, &opts).await
}

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

fn json_script_escape(value: &str) -> String {
    value
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026")
}

pub fn render_page(title: &str, page_script: &str, body_attrs: &str, body: &str) -> String {
    render_page_with_store_payload(title, page_script, body_attrs, body, "")
}

fn render_page_with_store_payload(
    title: &str,
    page_script: &str,
    body_attrs: &str,
    body: &str,
    store_payload: &str,
) -> String {
    let attrs = if body_attrs.is_empty() {
        String::new()
    } else {
        format!(" {body_attrs}")
    };
    let store_script = if store_payload.is_empty() {
        String::new()
    } else {
        format!(
            r#"<script type="application/json" data-argon-stores>{}</script>"#,
            json_script_escape(store_payload)
        )
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
        {store_script}
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

#[cfg(test)]
mod tests {
    use crate::api::threads::{
        MessageTemplateData, ProjectTemplateData, ThreadPageData, ThreadTemplateData,
        ToolCallResponse,
    };
    use crate::components::{
        shell_page_view::{
            RenderStores as ShellRenderStores, ShellPageData, ShellPageView, ShellPageViewServer,
        },
        threads_page_view::{
            RenderStores, ThreadPageData as ArgonThreadPageData, ThreadsPageView,
            ThreadsPageViewServer,
        },
        ui::Stores as UiStores,
    };

    struct TestPageServer(ArgonThreadPageData);
    struct TestShellServer(ShellPageData);

    impl ThreadsPageViewServer for TestPageServer {
        type Error = std::convert::Infallible;

        async fn load_thread_page(
            &self,
            _thread_id: &str,
        ) -> Result<ArgonThreadPageData, Self::Error> {
            Ok(self.0.clone())
        }
    }

    impl ShellPageViewServer for TestShellServer {
        type Error = std::convert::Infallible;

        async fn load_shell_page(&self, _page: &str) -> Result<ShellPageData, Self::Error> {
            Ok(self.0.clone())
        }
    }

    fn sample_data() -> ThreadPageData {
        ThreadPageData {
            thread_id: "thread-1".to_string(),
            current_title: "Current thread".to_string(),
            selected_model: "fast".to_string(),
            models: vec![crate::model_registry::ModelSummary {
                key: "fast".to_string(),
                slug: "fast-model".to_string(),
                display_name: "Fast".to_string(),
                description: "".to_string(),
                source: "config",
                provider: "test".to_string(),
                vision: false,
                reasoning_effort: Some("high".to_string()),
            }],
            running: true,
            projects: vec![ProjectTemplateData {
                id: "project-1".to_string(),
                title: "My <Project>".to_string(),
                threads: vec![ThreadTemplateData {
                    id: "thread-1".to_string(),
                    title: "Current thread".to_string(),
                    project_id: Some("project-1".to_string()),
                }],
            }],
            ungrouped_threads: vec![ThreadTemplateData {
                id: "thread-2".to_string(),
                title: "Loose thread".to_string(),
                project_id: None,
            }],
            messages: vec![
                MessageTemplateData {
                    id: "message-1".to_string(),
                    seq: 1,
                    created_at: 1_000,
                    role: "tool",
                    format: "markdown",
                    message_type: "tool_output",
                    tool_name: Some("Tool output".to_string()),
                    tool_call_id: None,
                    tool_calls: Vec::new(),
                    content: "done".to_string(),
                    thinking: None,
                    has_thinking: false,
                },
                MessageTemplateData {
                    id: "message-2".to_string(),
                    seq: 2,
                    created_at: 2_000,
                    role: "agent",
                    format: "html",
                    message_type: "agent",
                    tool_name: None,
                    tool_call_id: None,
                    tool_calls: Vec::new(),
                    content: "<p onclick=\"bad()\">hello & <strong>world</strong></p><script>bad</script>".to_string(),
                    thinking: None,
                    has_thinking: false,
                },
                MessageTemplateData {
                    id: "message-3".to_string(),
                    seq: 3,
                    created_at: 3_000,
                    role: "agent",
                    format: "markdown",
                    message_type: "agent",
                    tool_name: None,
                    tool_call_id: None,
                    tool_calls: Vec::new(),
                    content: "Here&#39;s Research &amp; Web".to_string(),
                    thinking: Some("A &amp; B".to_string()),
                    has_thinking: true,
                },
            ],
        }
    }

    #[tokio::test]
    async fn threads_page_renders_argon_server_data_and_shared_timeline() {
        let data = super::argon_thread_page_data(sample_data());
        let server = TestPageServer(data);
        let ui = UiStores::default();
        let thread_view = crate::components::thread_view::Stores::default();
        let side_panel = crate::components::side_panel::Stores::default();
        let stores = RenderStores {
            side_panel: &side_panel,
            thread_view: &thread_view,
            ui: &ui,
        };
        let page = ThreadsPageView::new("thread-1").attr("id", "threads-page");
        let html = page
            .render_document(
                &server,
                &stores,
                &crate::components::threads_page_view::DocumentOpts::default(),
            )
            .await
            .unwrap();

        assert!(html.contains(r#"<body><threads-page-view data-thread-id="thread-1""#));
        assert!(html.contains(r#"id="threads-page""#));
        assert!(html.contains("<nav><app-sidebar"));
        assert!(html.contains(r#"<template shadowrootmode="open">"#));
        assert!(html.contains("My &lt;Project&gt;"));
        assert!(html.contains("Loose thread"));
        assert!(html.contains(r#"data-activity-id="message-1""#));
        assert!(html.contains("<app-tool-activity"));
        assert!(html.contains("<app-work-group"));
        assert!(html.contains(r#"data-label="Worked for 2s""#));
        assert!(!html.contains(r#"data-message-id="message-2""#));
        assert!(
            html.contains(
                "&lt;p&gt;hello &amp;amp; &lt;strong&gt;world&lt;/strong&gt;&lt;/p&gt;bad"
            )
        );
        assert!(!html.contains("onclick"));
        assert!(!html.contains("<script>bad</script>"));
        assert!(html.contains(r#"data-message-id="message-3""#));
        assert!(html.contains("Here's Research &amp; Web"));
        assert!(html.contains("A &amp; B"));
        assert!(html.contains(r#"data-running="true""#));
        assert!(html.contains(r#"data-prompt"#));
        assert!(html.contains(r#"data-selected-model-label="Fast""#));
        assert!(html.contains(r#"data-selected-model-reasoning-effort="high""#));
        assert!(html.contains(r#"&quot;label&quot;:&quot;Fast&quot;"#));
        assert!(html.contains(r#"data-approval hidden"#));
        assert!(html.contains(r#"data-quiz hidden"#));
        assert!(html.contains(r#"<app-side-panel"#));
        assert!(html.contains("<app-file-explorer"));
        assert!(html.contains(r#"slot="files""#));
        assert!(html.contains("<app-subagent-view"));
        assert!(html.contains(r#"slot="subagents""#));
    }

    #[test]
    fn chat_timeline_merges_tool_calls_with_outputs_and_excludes_subagents() {
        let message = |id: &str, seq, role, content: &str| MessageTemplateData {
            id: id.to_string(),
            seq,
            created_at: seq as i64 * 1_000,
            role,
            format: "markdown",
            message_type: role,
            tool_name: None,
            tool_call_id: None,
            tool_calls: Vec::new(),
            content: content.to_string(),
            thinking: None,
            has_thinking: false,
        };
        let mut assistant = message("assistant", 1, "agent", "");
        assistant.tool_calls = vec![
            ToolCallResponse {
                id: "call-1".to_string(),
                name: "shell".to_string(),
                arguments: r#"{"command":"ls -la"}"#.to_string(),
            },
            ToolCallResponse {
                id: "call-2".to_string(),
                name: "collaboration.spawn_agent".to_string(),
                arguments: "{}".to_string(),
            },
        ];
        let mut output = message("output", 2, "tool", "files");
        output.tool_call_id = Some("call-1".to_string());
        let mut child = message("child", 3, "tool", "child result");
        child.tool_call_id = Some("call-2".to_string());

        let timeline = super::chat_timeline(vec![assistant, output, child]);

        assert_eq!(timeline.len(), 1);
        assert_eq!(timeline[0].id, "tool:call-1");
        assert_eq!(timeline[0].tool_name, "Ran command");
        assert_eq!(timeline[0].tool_detail, "ls -la");
        assert_eq!(timeline[0].content, "files");
    }

    #[tokio::test]
    async fn shell_pages_render_as_argon_components() {
        for (name, child) in [
            ("files", "app-file-browser"),
            ("automations", "app-automations"),
            ("archived", "app-archived-threads"),
            ("settings", "app-settings"),
        ] {
            let data = super::argon_thread_page_data(sample_data());
            let server = TestShellServer(ShellPageData {
                projects: data.projects,
                threads: data.threads,
            });
            let mut ui = UiStores::default();
            ui.sidebar.active_page = name.to_string();
            let stores = ShellRenderStores { ui: &ui };
            let html = ShellPageView::new(name)
                .attr("id", format!("{name}-page"))
                .render_document(
                    &server,
                    &stores,
                    &crate::components::shell_page_view::DocumentOpts::default(),
                )
                .await
                .unwrap();

            assert!(html.contains(&format!(r#"id="{name}-page""#)));
            assert!(html.contains(&format!("<{child}></{child}>")));
            assert!(html.contains("<nav><app-sidebar"));
        }
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
