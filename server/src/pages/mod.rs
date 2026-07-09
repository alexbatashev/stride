pub mod agent;
pub mod archived;
pub mod auth;
pub mod automations;
pub mod files;
pub mod settings;

use crate::api::threads::ThreadPageData;
use crate::components::{
    app_approval_bar::AppApprovalBar,
    app_button::AppButton,
    app_message::AppMessage,
    app_prompt_input::{AppPromptInput, Models},
    app_quiz_bar::AppQuizBar,
    app_sidebar::{
        AppSidebar, AppSidebarToggle, RenderStores as SidebarRenderStores, SidebarProject,
        SidebarThread,
    },
    auth_form::AuthForm,
    ui::Stores as UiStores,
};

fn ui_stores_for(data: &ThreadPageData, active_page: &str) -> UiStores {
    let mut stores = UiStores::default();
    stores.sidebar.active_thread = data.thread_id.clone();
    stores.sidebar.active_project = data
        .projects
        .iter()
        .flat_map(|project| project.threads.iter())
        .find(|thread| thread.id == data.thread_id)
        .and_then(|thread| thread.project_id.clone())
        .unwrap_or_default();
    stores.sidebar.active_page = active_page.to_string();
    stores
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

fn html_unescape_text(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut rest = value;

    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp + 1..];
        if let Some(semi) = after.find(';') {
            let entity = &after[..semi];
            if entity.len() <= 12
                && let Some(decoded) = decode_html_entity(entity)
            {
                out.push(decoded);
                rest = &after[semi + 1..];
                continue;
            }
        }
        out.push('&');
        rest = &rest[amp + 1..];
    }

    out.push_str(rest);
    out
}

fn decode_html_entity(entity: &str) -> Option<char> {
    match entity {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" | "#39" => Some('\''),
        _ => {
            let hex = entity
                .strip_prefix("#x")
                .or_else(|| entity.strip_prefix("#X"));
            if let Some(hex) = hex {
                return u32::from_str_radix(hex, 16).ok().and_then(char::from_u32);
            }
            entity
                .strip_prefix('#')
                .and_then(|dec| dec.parse::<u32>().ok())
                .and_then(char::from_u32)
        }
    }
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

fn render_sidebar(data: &ThreadPageData, ui_stores: &UiStores) -> String {
    let projects = data.projects.iter().map(|project| SidebarProject {
        id: project.id.clone(),
        title: project.title.clone(),
        threads: project
            .threads
            .iter()
            .map(|thread| SidebarThread {
                id: thread.id.clone(),
                title: thread.title.clone(),
            })
            .collect(),
    });
    let ungrouped = data.ungrouped_threads.iter().map(|thread| SidebarThread {
        id: thread.id.clone(),
        title: thread.title.clone(),
    });
    let sidebar = AppSidebar::new(projects, ungrouped);
    let stores = SidebarRenderStores { ui: ui_stores };
    format!("<nav>{}</nav>", sidebar.render(&stores))
}

fn render_messages(data: &ThreadPageData) -> String {
    if data.thread_id.is_empty() || data.messages.is_empty() {
        return r#"<div class="empty" data-empty>
                <h2>What are we working on?</h2>
                <p>Start a thread and S.T.R.I.D.E. will keep the context here.</p>
            </div>"#
            .to_string();
    }

    data.messages
        .iter()
        .map(|message| {
            let content = if message.format == "markdown" {
                html_unescape_text(&message.content)
            } else {
                message.content.clone()
            };
            let thinking = message
                .thinking
                .as_deref()
                .map(html_unescape_text)
                .unwrap_or_default();
            AppMessage::new(
                &message.id,
                message.seq as f64,
                message.role,
                message.message_type,
                message.format,
                content,
                thinking,
                message.tool_name.as_deref().unwrap_or_default().to_string(),
            )
            .render()
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
    let ui_stores = ui_stores_for(data, "");
    let sidebar = render_sidebar(data, &ui_stores);
    let sidebar_stores = SidebarRenderStores { ui: &ui_stores };
    let toggle = AppSidebarToggle::new("").render(&sidebar_stores);
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
        &AppPromptInput::new(
            false,
            data.running,
            placeholder,
            Vec::<Models>::new(),
            &data.selected_model,
        )
        .render(),
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
    let selected_model = html_escape(&data.selected_model);

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
        r#"id="threads-page" data-thread-id="{thread_id}" data-selected-model="{selected_model}" data-running="{running}""#,
        running = data.running,
    );
    render_page_with_store_payload(
        "S.T.R.I.D.E.",
        &agent::page_script(),
        &body_attrs,
        &body,
        &ui_stores.snapshot_json(),
    )
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
    let ui_stores = ui_stores_for(data, "files");
    let sidebar = render_sidebar(data, &ui_stores);
    let sidebar_stores = SidebarRenderStores { ui: &ui_stores };
    let toggle = AppSidebarToggle::new("").render(&sidebar_stores);
    let body = format!(
        r#"{FILES_STYLE}
{sidebar}
<main>
    <div class="mobile-bar">{toggle}</div>
    <app-file-browser></app-file-browser>
</main>
{NAVIGATE_SCRIPT}"#,
    );
    render_page_with_store_payload(
        "Files - S.T.R.I.D.E.",
        &files::page_script(),
        r#"id="files-page""#,
        &body,
        &ui_stores.snapshot_json(),
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
    let ui_stores = ui_stores_for(data, "automations");
    let sidebar = render_sidebar(data, &ui_stores);
    let sidebar_stores = SidebarRenderStores { ui: &ui_stores };
    let toggle = AppSidebarToggle::new("").render(&sidebar_stores);
    let body = format!(
        r#"{AUTOMATIONS_STYLE}
{sidebar}
<main>
    <div class="mobile-bar">{toggle}</div>
    <app-automations></app-automations>
</main>
{NAVIGATE_SCRIPT}"#,
    );
    render_page_with_store_payload(
        "Automations - S.T.R.I.D.E.",
        &automations::page_script(),
        r#"id="automations-page""#,
        &body,
        &ui_stores.snapshot_json(),
    )
}

pub fn render_archived_page(data: &ThreadPageData) -> String {
    let ui_stores = ui_stores_for(data, "archived");
    let sidebar = render_sidebar(data, &ui_stores);
    let sidebar_stores = SidebarRenderStores { ui: &ui_stores };
    let toggle = AppSidebarToggle::new("").render(&sidebar_stores);
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

    render_page_with_store_payload(
        "Archived - S.T.R.I.D.E.",
        &archived::page_script(),
        r#"id="archived-page""#,
        &body,
        &ui_stores.snapshot_json(),
    )
}

pub fn render_settings_page(data: &ThreadPageData) -> String {
    let ui_stores = ui_stores_for(data, "settings");
    let sidebar = render_sidebar(data, &ui_stores);
    let sidebar_stores = SidebarRenderStores { ui: &ui_stores };
    let toggle = AppSidebarToggle::new("").render(&sidebar_stores);

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

    render_page_with_store_payload(
        "Settings - S.T.R.I.D.E.",
        &settings::page_script(),
        r#"id="settings-page""#,
        &body,
        &ui_stores.snapshot_json(),
    )
}

#[cfg(test)]
mod tests {
    use crate::api::threads::{
        MessageTemplateData, ProjectTemplateData, ThreadPageData, ThreadTemplateData,
    };

    fn sample_data() -> ThreadPageData {
        ThreadPageData {
            thread_id: "thread-1".to_string(),
            current_title: "Current thread".to_string(),
            selected_model: "fast".to_string(),
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
                    role: "tool",
                    format: "markdown",
                    message_type: "tool_output",
                    tool_name: Some("Tool output".to_string()),
                    content: "done".to_string(),
                    thinking: None,
                    has_thinking: false,
                },
                MessageTemplateData {
                    id: "message-2".to_string(),
                    seq: 2,
                    role: "agent",
                    format: "html",
                    message_type: "agent",
                    tool_name: None,
                    content: "hello & <world>".to_string(),
                    thinking: None,
                    has_thinking: false,
                },
                MessageTemplateData {
                    id: "message-3".to_string(),
                    seq: 3,
                    role: "agent",
                    format: "markdown",
                    message_type: "agent",
                    tool_name: None,
                    content: "Here&#39;s Research &amp; Web".to_string(),
                    thinking: Some("A &amp; B".to_string()),
                    has_thinking: true,
                },
            ],
        }
    }

    #[test]
    fn threads_page_renders_shell_components_and_messages() {
        let html = super::render_threads_page(&sample_data());

        assert!(
            html.contains(
                r#"<body id="threads-page" data-thread-id="thread-1" data-selected-model="fast" data-running="true">"#
            )
        );
        // Server-side shadow DOM for the chrome that should paint before JS.
        assert!(html.contains("<nav><app-sidebar"));
        assert!(html.contains(r#"<template shadowrootmode="open">"#));
        assert!(html.contains(r#""activeThread":"thread-1""#));
        assert!(html.contains(r#""activeProject":"project-1""#));
        assert!(html.contains("My &lt;Project&gt;"));
        assert!(html.contains("Loose thread"));
        // Messages arrive as hydrated app-message components. Tool output is
        // folded into a spoiler, so its content rides in the hydration
        // attribute; agent text paints inside the shadow DOM markdown view.
        assert!(html.contains(r#"data-message-id="message-1""#));
        assert!(html.contains(r#"data-kind="tool_output""#));
        assert!(html.contains(r#"data-message-id="message-2""#));
        assert!(html.contains("hello &amp; &lt;world&gt;"));
        assert!(html.contains(r#"data-message-id="message-3""#));
        assert!(html.contains("Here's Research &amp; Web"));
        assert!(html.contains("A &amp; B"));
        assert!(!html.contains("Here&amp;#39;s"));
        assert!(!html.contains("&amp;amp; Web"));
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
        assert!(html.contains(r#""activePage":"files""#));
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
        assert!(html.contains(r#""activePage":"settings""#));
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
