pub mod agent;
pub mod auth;
pub mod automations;
pub mod files;
pub mod settings;

use crate::api::threads::ThreadPageData;
use crate::components::{
    app_approval_bar::AppApprovalBar,
    app_button::AppButton,
    app_message::AppMessage,
    app_prompt_input::AppPromptInput,
    app_quiz_bar::AppQuizBar,
    app_sidebar::{AppSidebar, AppSidebarToggle, SidebarProject, SidebarThread},
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

pub fn render_page(title: &str, page_script: &str, body_attrs: &str, body: &str) -> String {
    let attrs = if body_attrs.is_empty() {
        String::new()
    } else {
        format!(" {body_attrs}")
    };
    format!(
        r#"<!doctype html>
<html lang="en">
    <head>
        <meta charset="utf-8" />
        <meta name="viewport" content="width=device-width, initial-scale=1" />
        <title>{title}</title>
        <link rel="stylesheet" href="/static/common.css" />
        <link rel="modulepreload" href="/static/components.js">
        <script type="module" src="/static/components.js"></script>
        <script type="module" src="/static/api.js"></script>
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
    );
    format!("<nav>{sidebar}</nav>")
}

fn render_messages(data: &ThreadPageData) -> String {
    if data.thread_id.is_empty() || data.messages.is_empty() {
        return r#"<div class="empty" data-empty>
                <h2>What are we working on?</h2>
                <p>Start a thread and Friday will keep the context here.</p>
            </div>"#
            .to_string();
    }

    data.messages
        .iter()
        .map(|message| {
            AppMessage::new(
                &message.id,
                message.seq as f64,
                message.role,
                message.message_type,
                html_escape(&message.content),
                message
                    .thinking
                    .as_deref()
                    .map(html_escape)
                    .unwrap_or_default(),
                message
                    .tool_name
                    .as_deref()
                    .map(html_escape)
                    .unwrap_or_default(),
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
    let sidebar = render_sidebar(data, false, false, false);
    let toggle = AppSidebarToggle::new("").render();
    let files_button = with_attrs(
        &AppButton::new().render(),
        r#"variant="ghost" size="sm" class="files-button" data-action="files""#,
    );
    // The slot content goes in the host's light DOM, after the shadow template.
    let files_button = files_button.replacen("</app-button>", "Files</app-button>", 1);
    let messages = render_messages(data);
    let placeholder = if data.thread_id.is_empty() {
        "Ask Friday anything"
    } else {
        "Message Friday"
    };
    let prompt = with_attrs(
        &AppPromptInput::new(false, data.running, placeholder).render(),
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
    render_page("Friday", agent::PAGE_SCRIPT, &body_attrs, &body)
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
    let sidebar = render_sidebar(data, true, false, false);
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
        "Files - Friday",
        files::PAGE_SCRIPT,
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
    let sidebar = render_sidebar(data, false, true, false);
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
        "Automations - Friday",
        automations::PAGE_SCRIPT,
        r#"id="automations-page""#,
        &body,
    )
}

pub fn render_settings_page(data: &ThreadPageData) -> String {
    let sidebar = render_sidebar(data, false, false, true);
    let toggle = AppSidebarToggle::new("").render();
    let disconnect_button = with_attrs(
        &AppButton::new().render(),
        r#"variant="destructive" size="sm" data-action="disconnect""#,
    )
    .replacen("</app-button>", "Disconnect</app-button>", 1);

    let body = format!(
        r#"<style>
    #settings-page > main > header {{
        display: none;
    }}

    #settings-page .settings-content {{
        box-sizing: border-box;
        margin: 0 auto;
        max-width: 760px;
        padding: 32px 24px;
        width: 100%;
    }}

    #settings-page h1 {{
        color: var(--foreground);
        font-size: 26px;
        line-height: 1.2;
        margin: 0 0 8px;
    }}

    #settings-page .intro,
    #settings-page .muted {{
        color: var(--muted-foreground);
        font-size: 14px;
        line-height: 1.5;
        margin: 0;
    }}

    #settings-page .section {{
        border-top: 1px solid var(--border);
        margin-top: 24px;
        padding-top: 24px;
    }}

    #settings-page .section h2 {{
        color: var(--foreground);
        font-size: 16px;
        margin: 0 0 8px;
    }}

    #settings-page .status {{
        color: var(--foreground);
        font-size: 14px;
        margin: 12px 0;
    }}

    #settings-page .actions {{
        display: flex;
        flex-wrap: wrap;
        gap: 8px;
        margin-top: 16px;
    }}

    #settings-page .telegram-widget {{
        margin-top: 16px;
        min-height: 40px;
    }}

    #settings-page .telegram-widget:empty {{
        display: none;
    }}

    #settings-page .error {{
        color: var(--destructive);
        font-size: 13px;
        margin-top: 12px;
    }}

    #settings-page .error:empty {{
        display: none;
    }}

    #settings-page .email-list {{
        display: grid;
        gap: 8px;
        margin-top: 16px;
    }}

    #settings-page .email-account {{
        align-items: center;
        border: 1px solid var(--border);
        border-radius: 10px;
        display: flex;
        gap: 16px;
        justify-content: space-between;
        padding: 12px;
    }}

    #settings-page .email-account strong,
    #settings-page .email-account span {{
        display: block;
    }}

    #settings-page .email-account span {{
        color: var(--muted-foreground);
        font-size: 12px;
        margin-top: 3px;
    }}

    #settings-page .imap-form {{
        border: 1px solid var(--border);
        border-radius: 12px;
        display: grid;
        gap: 12px;
        margin-top: 20px;
        padding: 16px;
    }}

    #settings-page .form-grid {{
        display: grid;
        gap: 12px;
        grid-template-columns: 1fr 1fr;
    }}

    #settings-page label {{
        color: var(--foreground);
        display: grid;
        font-size: 13px;
        gap: 5px;
    }}

    #settings-page input {{
        background: var(--background);
        border: 1px solid var(--border);
        border-radius: 8px;
        box-sizing: border-box;
        color: var(--foreground);
        font: inherit;
        min-height: 38px;
        padding: 8px 10px;
        width: 100%;
    }}

    #settings-page details summary {{
        color: var(--foreground);
        cursor: pointer;
        font-size: 13px;
    }}

    #settings-page details .form-grid {{
        margin-top: 12px;
    }}

    #settings-page button {{
        background: var(--primary);
        border: 1px solid var(--primary);
        border-radius: 8px;
        color: var(--primary-foreground);
        cursor: pointer;
        font: inherit;
        min-height: 36px;
        padding: 0 12px;
    }}

    #settings-page button:disabled {{
        cursor: wait;
        opacity: .6;
    }}

    #settings-page button.danger-button {{
        background: transparent;
        border-color: var(--border);
        color: var(--destructive);
    }}

    @media (max-width: 767px) {{
        #settings-page > main > header {{
            align-items: center;
            border-bottom: 1px solid var(--border);
            box-sizing: border-box;
            display: flex;
            padding: 8px 12px;
        }}

        #settings-page .form-grid {{
            grid-template-columns: 1fr;
        }}
    }}
</style>
{sidebar}
<main>
    <header>{toggle}</header>
    <section class="settings-content">
        <h1>Settings</h1>
        <p class="intro">Manage account integrations.</p>
        <section class="section" data-telegram>
            <h2>Telegram</h2>
            <p class="muted">Connect your Telegram account with the Friday bot.</p>
            <p class="status" data-telegram-status>Loading...</p>
            <div class="telegram-widget" data-telegram-widget></div>
            <div class="actions">
                {disconnect_button}
            </div>
            <div class="error" data-telegram-error></div>
        </section>
        <section class="section" data-email>
            <h2>Email</h2>
            <p class="muted">Connect one or more TLS IMAP accounts. Friday can read incoming and sent mail and save reply-all drafts. It cannot send email.</p>
            <div class="email-list" data-email-list></div>
            <p class="status" data-email-empty>There are no IMAP accounts yet.</p>
            <form class="imap-form" data-email-form>
                <strong>Add IMAP server</strong>
                <div class="form-grid">
                    <label>Account name<input name="name" required placeholder="Work" autocomplete="off" /></label>
                    <label>Email address<input name="email" type="email" required placeholder="you@example.com" autocomplete="email" /></label>
                    <label>IMAP host<input name="host" required placeholder="imap.example.com" autocomplete="off" /></label>
                    <label>Port<input name="port" type="number" min="1" max="65535" value="993" required /></label>
                    <label>Username<input name="username" required placeholder="you@example.com" autocomplete="username" /></label>
                    <label>Password or app password<input name="password" type="password" required autocomplete="new-password" /></label>
                </div>
                <details>
                    <summary>Mailbox names</summary>
                    <div class="form-grid">
                        <label>Inbox<input name="inbox_mailbox" value="INBOX" required /></label>
                        <label>Sent<input name="sent_mailbox" value="Sent" required /></label>
                        <label>Drafts<input name="drafts_mailbox" value="Drafts" required /></label>
                    </div>
                </details>
                <p class="muted">The connection is verified before it is saved. Credentials are encrypted at rest.</p>
                <div><button type="submit">Add account</button></div>
                <div class="error" data-email-error></div>
            </form>
        </section>
    </section>
</main>
{NAVIGATE_SCRIPT}"#,
    );

    render_page(
        "Settings - Friday",
        settings::PAGE_SCRIPT,
        r#"id="settings-page""#,
        &body,
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
                    message_type: "agent",
                    tool_name: None,
                    content: "hello & <world>".to_string(),
                    thinking: None,
                    has_thinking: false,
                },
            ],
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
        assert!(html.contains("Ask Friday anything"));
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
    fn settings_page_renders_integration_controls() {
        let html = super::render_settings_page(&sample_data());

        assert!(html.contains(r#"<body id="settings-page">"#));
        assert!(html.contains("Telegram"));
        assert!(html.contains("data-telegram-widget"));
        assert!(html.contains(r#"data-action="disconnect""#));
        assert!(html.contains("Add IMAP server"));
        assert!(html.contains("data-email-form"));
        assert!(html.contains("cannot send email"));
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
