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
        r##"<style>
    #settings-page > main {{
        background:
            radial-gradient(circle at top left, color-mix(in srgb, var(--primary) 9%, transparent), transparent 34rem),
            var(--background);
        min-height: 100vh;
    }}

    #settings-page > main > header {{
        display: none;
    }}

    #settings-page .settings-shell {{
        box-sizing: border-box;
        margin: 0 auto;
        max-width: 1180px;
        padding: 36px 28px 56px;
        width: 100%;
    }}

    #settings-page .settings-hero {{
        align-items: end;
        display: grid;
        gap: 20px;
        grid-template-columns: 1fr auto;
        margin-bottom: 28px;
    }}

    #settings-page .eyebrow {{
        color: var(--muted-foreground);
        font-size: 12px;
        font-weight: 600;
        letter-spacing: .12em;
        margin: 0 0 10px;
        text-transform: uppercase;
    }}

    #settings-page h1 {{
        color: var(--foreground);
        font-size: clamp(32px, 5vw, 52px);
        letter-spacing: -0.055em;
        line-height: .95;
        margin: 0;
    }}

    #settings-page .intro,
    #settings-page .muted,
    #settings-page .field-hint {{
        color: var(--muted-foreground);
        font-size: 14px;
        line-height: 1.55;
        margin: 0;
    }}

    #settings-page .intro {{
        margin-top: 14px;
        max-width: 660px;
    }}

    #settings-page .hero-actions {{
        align-items: center;
        display: flex;
        gap: 10px;
    }}

    #settings-page .settings-layout {{
        align-items: start;
        display: grid;
        gap: 24px;
        grid-template-columns: 220px minmax(0, 1fr);
    }}

    #settings-page .settings-nav {{
        background: color-mix(in srgb, var(--card) 82%, transparent);
        border: 1px solid var(--border);
        border-radius: 18px;
        box-shadow: 0 18px 50px rgb(0 0 0 / 6%);
        padding: 10px;
        position: sticky;
        top: 24px;
    }}

    #settings-page .settings-nav a {{
        align-items: center;
        border-radius: 12px;
        color: var(--muted-foreground);
        display: flex;
        font-size: 14px;
        font-weight: 500;
        gap: 10px;
        padding: 10px 12px;
        text-decoration: none;
    }}

    #settings-page .settings-nav a[aria-current="page"],
    #settings-page .settings-nav a:hover {{
        background: var(--muted);
        color: var(--foreground);
    }}

    #settings-page .nav-dot {{
        background: currentColor;
        border-radius: 999px;
        height: 6px;
        opacity: .7;
        width: 6px;
    }}

    #settings-page .settings-content {{
        display: grid;
        gap: 18px;
        min-width: 0;
    }}

    #settings-page .overview-grid {{
        display: grid;
        gap: 14px;
        grid-template-columns: repeat(3, minmax(0, 1fr));
    }}

    #settings-page .metric-card,
    #settings-page .section {{
        background: color-mix(in srgb, var(--card) 92%, transparent);
        border: 1px solid var(--border);
        border-radius: 20px;
        box-shadow: 0 18px 50px rgb(0 0 0 / 5%);
    }}

    #settings-page .metric-card {{
        display: grid;
        gap: 8px;
        min-height: 112px;
        padding: 18px;
    }}

    #settings-page .metric-icon {{
        align-items: center;
        background: var(--primary);
        border-radius: 14px;
        color: var(--primary-foreground);
        display: inline-flex;
        font-size: 18px;
        height: 38px;
        justify-content: center;
        width: 38px;
    }}

    #settings-page .metric-card strong {{
        color: var(--foreground);
        font-size: 15px;
    }}

    #settings-page .section {{
        overflow: hidden;
    }}

    #settings-page .section-header {{
        align-items: start;
        border-bottom: 1px solid var(--border);
        display: grid;
        gap: 12px;
        grid-template-columns: 1fr auto;
        padding: 22px 24px;
    }}

    #settings-page .section-kicker {{
        color: var(--muted-foreground);
        font-size: 12px;
        font-weight: 600;
        letter-spacing: .08em;
        margin: 0 0 7px;
        text-transform: uppercase;
    }}

    #settings-page h2 {{
        color: var(--foreground);
        font-size: 22px;
        letter-spacing: -0.025em;
        line-height: 1.2;
        margin: 0 0 6px;
    }}

    #settings-page .status-pill {{
        align-items: center;
        background: var(--muted);
        border: 1px solid var(--border);
        border-radius: 999px;
        color: var(--foreground);
        display: inline-flex;
        font-size: 12px;
        font-weight: 600;
        gap: 8px;
        padding: 6px 10px;
        white-space: nowrap;
    }}

    #settings-page .status-pill::before {{
        background: #22c55e;
        border-radius: 999px;
        content: "";
        height: 7px;
        width: 7px;
    }}

    #settings-page .section-body {{
        display: grid;
        gap: 20px;
        padding: 24px;
    }}

    #settings-page .telegram-panel {{
        align-items: center;
        background: linear-gradient(135deg, color-mix(in srgb, var(--primary) 12%, transparent), transparent), var(--muted);
        border: 1px solid var(--border);
        border-radius: 16px;
        display: grid;
        gap: 18px;
        grid-template-columns: 1fr auto;
        padding: 18px;
    }}

    #settings-page .status {{
        color: var(--foreground);
        font-size: 15px;
        font-weight: 600;
        margin: 0 0 4px;
    }}

    #settings-page .telegram-widget:empty,
    #settings-page .error:empty {{
        display: none;
    }}

    #settings-page .error {{
        background: var(--destructive-muted, rgb(220 38 38 / 10%));
        border: 1px solid color-mix(in srgb, var(--destructive) 22%, transparent);
        border-radius: 12px;
        color: var(--destructive);
        font-size: 13px;
        padding: 10px 12px;
    }}

    #settings-page .resource-list {{
        display: grid;
        gap: 10px;
    }}

    #settings-page .empty-state {{
        background: var(--muted);
        border: 1px dashed var(--border);
        border-radius: 14px;
        color: var(--muted-foreground);
        font-size: 14px;
        margin: 0;
        padding: 18px;
    }}

    #settings-page .integration-account {{
        align-items: center;
        background: var(--background);
        border: 1px solid var(--border);
        border-radius: 14px;
        display: flex;
        gap: 16px;
        justify-content: space-between;
        padding: 14px;
    }}

    #settings-page .integration-account strong,
    #settings-page .integration-account span {{
        display: block;
    }}

    #settings-page .integration-account strong {{
        color: var(--foreground);
        font-size: 14px;
    }}

    #settings-page .integration-account span {{
        color: var(--muted-foreground);
        font-size: 12px;
        margin-top: 4px;
    }}

    #settings-page .settings-form {{
        background: var(--background);
        border: 1px solid var(--border);
        border-radius: 16px;
        display: grid;
        gap: 16px;
        padding: 18px;
    }}

    #settings-page .form-title {{
        align-items: center;
        color: var(--foreground);
        display: flex;
        font-size: 15px;
        font-weight: 650;
        justify-content: space-between;
    }}

    #settings-page .form-grid {{
        display: grid;
        gap: 14px;
        grid-template-columns: repeat(2, minmax(0, 1fr));
    }}

    #settings-page label {{
        color: var(--foreground);
        display: grid;
        font-size: 13px;
        font-weight: 500;
        gap: 7px;
    }}

    #settings-page input,
    #settings-page textarea {{
        background: var(--background);
        border: 1px solid var(--input, var(--border));
        border-radius: 10px;
        box-sizing: border-box;
        color: var(--foreground);
        font: inherit;
        min-height: 38px;
        outline: none;
        padding: 9px 11px;
        width: 100%;
    }}

    #settings-page input:focus,
    #settings-page textarea:focus {{
        border-color: var(--ring);
        box-shadow: 0 0 0 3px var(--ring-shadow, rgb(24 24 27 / 12%));
    }}

    #settings-page textarea {{
        min-height: 92px;
        resize: vertical;
    }}

    #settings-page details {{
        background: var(--muted);
        border-radius: 12px;
        padding: 12px;
    }}

    #settings-page details summary {{
        color: var(--foreground);
        cursor: pointer;
        font-size: 13px;
        font-weight: 600;
    }}

    #settings-page details .form-grid {{
        margin-top: 12px;
    }}

    #settings-page button {{
        background: var(--primary);
        border: 1px solid var(--primary);
        border-radius: 10px;
        color: var(--primary-foreground);
        cursor: pointer;
        font: inherit;
        font-size: 14px;
        font-weight: 600;
        min-height: 38px;
        padding: 0 14px;
    }}

    #settings-page button:hover {{
        background: var(--primary-hover, var(--primary));
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

    @media (max-width: 980px) {{
        #settings-page .settings-layout,
        #settings-page .settings-hero {{
            grid-template-columns: 1fr;
        }}

        #settings-page .settings-nav {{
            display: flex;
            gap: 6px;
            overflow-x: auto;
            position: static;
        }}

        #settings-page .settings-nav a {{
            white-space: nowrap;
        }}

        #settings-page .overview-grid {{
            grid-template-columns: 1fr;
        }}
    }}

    @media (max-width: 767px) {{
        #settings-page > main > header {{
            align-items: center;
            border-bottom: 1px solid var(--border);
            box-sizing: border-box;
            display: flex;
            padding: 8px 12px;
        }}

        #settings-page .settings-shell {{
            padding: 24px 14px 40px;
        }}

        #settings-page .form-grid,
        #settings-page .section-header,
        #settings-page .telegram-panel {{
            grid-template-columns: 1fr;
        }}
    }}
</style>
{sidebar}
<main>
    <header>{toggle}</header>
    <section class="settings-shell">
        <div class="settings-hero">
            <div>
                <p class="eyebrow">Workspace settings</p>
                <h1>Command center for Friday.</h1>
                <p class="intro">Connect the channels and tool servers Friday can use to read context, draft replies, and act on your behalf.</p>
            </div>
            <div class="hero-actions">
                <a class="status-pill" href="#telegram">Live configuration</a>
            </div>
        </div>
        <div class="settings-layout">
            <nav class="settings-nav" aria-label="Settings sections">
                <a href="#telegram" aria-current="page"><span class="nav-dot"></span>Telegram</a>
                <a href="#email"><span class="nav-dot"></span>Email</a>
                <a href="#mcp"><span class="nav-dot"></span>MCP servers</a>
            </nav>
            <div class="settings-content">
                <div class="overview-grid" aria-label="Integration overview">
                    <article class="metric-card">
                        <span class="metric-icon">✦</span>
                        <strong>Personal channels</strong>
                        <p class="muted">Telegram and IMAP give Friday real conversations to help with.</p>
                    </article>
                    <article class="metric-card">
                        <span class="metric-icon">⌘</span>
                        <strong>Agent tools</strong>
                        <p class="muted">Remote MCP servers extend what cloud agents can do safely.</p>
                    </article>
                    <article class="metric-card">
                        <span class="metric-icon">◆</span>
                        <strong>Encrypted secrets</strong>
                        <p class="muted">Credentials are verified before saving and kept encrypted at rest.</p>
                    </article>
                </div>
                <section class="section" id="telegram" data-telegram>
                    <div class="section-header">
                        <div>
                            <p class="section-kicker">Messaging</p>
                            <h2>Telegram</h2>
                            <p class="muted">Connect the Friday bot to receive updates and approve work from chat.</p>
                        </div>
                        <span class="status-pill">Bot channel</span>
                    </div>
                    <div class="section-body">
                        <div class="telegram-panel">
                            <div>
                                <p class="status" data-telegram-status>Loading...</p>
                                <p class="muted">Use Telegram login to bind this browser session to your account.</p>
                            </div>
                            <div class="telegram-widget" data-telegram-widget></div>
                        </div>
                        <div class="hero-actions">
                            {disconnect_button}
                        </div>
                        <div class="error" data-telegram-error></div>
                    </div>
                </section>
                <section class="section" id="email" data-email>
                    <div class="section-header">
                        <div>
                            <p class="section-kicker">Inbox context</p>
                            <h2>Email</h2>
                            <p class="muted">Connect TLS IMAP accounts. Friday can read incoming and sent mail and save reply-all drafts. It cannot send email.</p>
                        </div>
                        <span class="status-pill">IMAP only</span>
                    </div>
                    <div class="section-body">
                        <div class="resource-list" data-email-list></div>
                        <p class="empty-state" data-email-empty>No IMAP accounts yet. Add one below to unlock email automations.</p>
                        <form class="settings-form" data-email-form>
                            <div class="form-title">Add IMAP server <span class="muted">Verified before save</span></div>
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
                            <p class="field-hint">Credentials are encrypted at rest. Existing mail is ignored when automations start.</p>
                            <div><button type="submit">Add account</button></div>
                            <div class="error" data-email-error></div>
                        </form>
                    </div>
                </section>
                <section class="section" id="mcp" data-mcp>
                    <div class="section-header">
                        <div>
                            <p class="section-kicker">Tooling</p>
                            <h2>MCP servers</h2>
                            <p class="muted">Add remote HTTP MCP servers for your agents. Tools from these servers are loaded with the global MCP servers.</p>
                        </div>
                        <span class="status-pill">Streamable HTTP</span>
                    </div>
                    <div class="section-body">
                        <div class="resource-list" data-mcp-list></div>
                        <p class="empty-state" data-mcp-empty>No custom MCP servers yet. Add a trusted server to expand Friday's toolbelt.</p>
                        <form class="settings-form" data-mcp-form>
                            <div class="form-title">Add MCP server <span class="muted">Authorization is hidden after save</span></div>
                            <div class="form-grid">
                                <label>Name<input name="name" required placeholder="deepwiki" autocomplete="off" pattern="[A-Za-z][A-Za-z0-9_]{{1,47}}" /></label>
                                <label>URL<input name="url" type="url" required placeholder="https://mcp.example.com/mcp" autocomplete="off" /></label>
                                <label>Bearer token<input name="bearer_token" type="password" autocomplete="new-password" /></label>
                            </div>
                            <label>Headers JSON<textarea name="headers_json" placeholder='{{"X-Tenant":"acme"}}'></textarea></label>
                            <p class="field-hint">Only Streamable HTTP MCP servers are supported here. Keep headers narrow and avoid broad production credentials.</p>
                            <div><button type="submit">Add server</button></div>
                            <div class="error" data-mcp-error></div>
                        </form>
                    </div>
                </section>
            </div>
        </div>
    </section>
</main>
{NAVIGATE_SCRIPT}"##,
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
        assert!(html.contains("MCP servers"));
        assert!(html.contains("data-mcp-form"));
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
