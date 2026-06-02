pub mod agent;
pub mod auth;
pub mod files;

use handlebars::{Handlebars, html_escape};
use serde_json::Value;

pub fn get_templates() -> anyhow::Result<Handlebars<'static>> {
    let mut hb = Handlebars::new();
    hb.register_template_string("base", BASE_TEMPLATE)?;
    hb.register_template_string("auth", AUTH_TEMPLATE)?;
    let sidebar = SIDEBAR_PARTIAL
        .replace(
            "<!-- ICON:bot-message-square -->",
            &crate::components2::bot_message_square::IconBotMessageSquare {}.render(),
        )
        .replace(
            "<!-- ICON:files -->",
            &crate::components2::files::IconFiles {}.render(),
        )
        .replace(
            "<!-- ICON:workflow -->",
            &crate::components2::workflow::IconWorkflow {}.render(),
        );
    hb.register_template_string("sidebar", &sidebar)?;
    hb.register_template_string("threads", THREADS_TEMPLATE)?;
    hb.register_template_string("files", FILES_TEMPLATE)?;
    Ok(hb)
}

pub fn render_page(
    hb: &Handlebars,
    title: &str,
    page_script: &str,
    template: &str,
    data: &Value,
) -> String {
    let body = hb.render(template, data).unwrap();
    let body_attrs = if template == "threads" {
        let thread_id = data
            .get("thread_id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let running = data
            .get("running")
            .and_then(Value::as_bool)
            .unwrap_or_default();
        let thread_id = html_escape(thread_id);
        format!(r#"id="threads-page" data-thread-id="{thread_id}" data-running="{running}""#)
    } else if template == "files" {
        r#"id="files-page""#.to_string()
    } else {
        String::new()
    };
    hb.render(
        "base",
        &serde_json::json!({
            "title": title,
            "page_script": page_script,
            "body": body,
            "body_attrs": body_attrs,
        }),
    )
    .unwrap()
}

const BASE_TEMPLATE: &str = r#"<!doctype html>
<html lang="en">
    <head>
        <meta charset="utf-8" />
        <meta name="viewport" content="width=device-width, initial-scale=1" />
        <title>{{title}}</title>
        <script type="importmap">{"imports": {"lit": "/static/lit.js"}}</script>
        <link rel="stylesheet" href="/static/common.css" />
        <link rel="modulepreload" href="/static/lit.js">
        <link rel="modulepreload" href="/static/components2.js">
        <link rel="modulepreload" href="/static/components.js">
        <script type="module" src="/static/components2.js"></script>
        <script type="module" src="/static/api.js"></script>
        <script type="module" src="/static/components.js"></script>
        {{{page_script}}}
    </head>
    <body{{#if body_attrs}} {{{body_attrs}}}{{/if}}>
        {{{body}}}
    </body>
</html>"#;

const AUTH_TEMPLATE: &str = r#"<auth-form mode="{{mode}}"></auth-form>
<script type="module">
    document.addEventListener('auth-success', () => { window.location.href = '/threads'; });
    document.addEventListener('auth-mode-change', (e) => { window.location.href = '/auth/' + e.detail.mode; });
</script>"#;

const SIDEBAR_PARTIAL: &str = r#"<nav>
    <app-sidebar>
        <div slot="header" class="brand">
            <span class="mark">F</span><strong>Friday</strong>
            <app-sidebar-toggle class="sidebar-brand-toggle" brand="F"></app-sidebar-toggle>
        </div>
        <app-sidebar-nav-item target="/threads" data-action="new-thread">
            <span slot="icon"><!-- ICON:bot-message-square --></span>
            New task
        </app-sidebar-nav-item>
        <app-sidebar-nav-item target="/files"{{#if files_active}} active{{/if}}>
            <span slot="icon"><!-- ICON:files --></span>
            Files
        </app-sidebar-nav-item>
        <app-sidebar-nav-item target="/threads">
            <span slot="icon"><!-- ICON:workflow --></span>
            Automations
        </app-sidebar-nav-item>
        <div data-sidebar-list>
            {{#each projects}}
                <app-sidebar-group title="{{title}}" data-project-id="{{id}}">
                    {{#each threads}}
                        <app-sidebar-group-item target="/threads/{{id}}" {{#if active}}active{{/if}} data-thread-id="{{id}}">
                            <span class="thread-label">{{title}}</span>
                        </app-sidebar-group-item>
                    {{/each}}
                </app-sidebar-group>
            {{/each}}
            {{#if ungrouped_threads}}
                <app-sidebar-group title="Threads">
                    {{#each ungrouped_threads}}
                        <app-sidebar-group-item target="/threads/{{id}}" {{#if active}}active{{/if}} data-thread-id="{{id}}">
                            <span class="thread-label">{{title}}</span>
                        </app-sidebar-group-item>
                    {{/each}}
                </app-sidebar-group>
            {{/if}}
        </div>
        <div slot="footer" class="sidebar-footer">
            <app-button class="sidebar-action" variant="ghost" data-action="new-project">+ New project</app-button>
            <app-button class="sidebar-action" variant="secondary" data-action="logout">Log out</app-button>
        </div>
    </app-sidebar>
</nav>"#;

const THREADS_TEMPLATE: &str = r#"<style>
    #threads-page .brand {
        align-items: center;
        display: flex;
        gap: 10px;
        margin-bottom: 10px;
        padding: 4px;
    }

    #threads-page .mark {
        align-items: center;
        background: var(--primary);
        border-radius: 8px;
        color: var(--primary-foreground);
        display: inline-flex;
        font-size: 13px;
        font-weight: 700;
        height: 32px;
        justify-content: center;
        width: 32px;
    }

    #threads-page .brand strong {
        color: var(--foreground);
        flex: 1;
        font-size: 14px;
        font-weight: 650;
        min-width: 0;
    }

    #threads-page .mobile-sidebar-toggle {
        display: none;
    }

    #threads-page app-sidebar[status="collapsed"] .brand {
        justify-content: center;
        padding: 8px;
    }

    #threads-page app-sidebar[status="collapsed"] .brand .mark,
    #threads-page app-sidebar[status="collapsed"] .brand strong {
        display: none;
    }

    #threads-page app-sidebar[status="collapsed"] [data-sidebar-list] {
        display: none;
    }

    #threads-page .thread-label {
        display: block;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
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

    #threads-page .sidebar-footer {
        display: flex;
        flex-direction: column;
        gap: 4px;
        padding: 8px;
        width: 100%;
        box-sizing: border-box;
    }

    #threads-page .sidebar-action {
        width: 100%;
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

    #threads-page .project-actions {
        display: none;
        gap: 2px;
        margin-left: auto;
    }

    #threads-page .project-action-btn {
        background: transparent;
        border: 0;
        border-radius: 4px;
        color: var(--muted-foreground);
        cursor: pointer;
        font-size: 12px;
        height: 20px;
        line-height: 1;
        padding: 0 4px;
    }

    #threads-page .project-action-btn:hover {
        background: var(--accent);
        color: var(--accent-foreground);
    }

    #threads-page > main > header {
        border-bottom: 1px solid var(--border);
        box-sizing: border-box;
        display: flex;
        justify-content: flex-end;
    }

    #threads-page .toolbar-spacer {
        flex: 1;
    }

    #threads-page .files-button {
        min-width: 72px;
    }

    @media (max-width: 767px) {
        #threads-page .mobile-sidebar-toggle {
            display: inline-flex;
        }

        #threads-page .sidebar-brand-toggle {
            display: none;
        }

        #threads-page > main > header {
            justify-content: space-between;
        }
    }
</style>
{{> sidebar}}
<main>
    <header>
        <app-sidebar-toggle class="mobile-sidebar-toggle"></app-sidebar-toggle>
        <span class="toolbar-spacer"></span>
        <app-button class="files-button" variant="ghost" size="sm" data-action="files">Files</app-button>
        <span data-current-title hidden>{{current_title}}</span>
    </header>
    <section class="content">
        <div class="wrapper" data-messages>
            {{#if messages}}
                {{#each messages}}
                    <app-message
                        message_id="{{id}}"
                        type="{{message_type}}"
                        {{#if tool_name}}tool_names="{{tool_name}}"{{/if}}
                        {{#if has_thinking}}with_thinking="true"{{/if}}
                        data-message-id="{{id}}"
                        data-seq="{{seq}}"
                        data-role="{{role}}"
                    >
                        {{#if thinking}}<span slot="thinking" data-thinking>{{thinking}}</span>{{/if}}
                        <span data-content>{{content}}</span>
                    </app-message>
                {{/each}}
            {{else}}
                <div class="empty" data-empty>
                    <h2>What are we working on?</h2>
                    <p>Start a thread and Friday will keep the context here.</p>
                </div>
            {{/if}}
        </div>
    </section>
    <app-prompt-input
        style="margin: auto"
        data-prompt
        placeholder="{{#if thread_id}}Message Friday{{else}}Ask Friday anything{{/if}}"
        {{#if running}}disabled{{/if}}
    ></app-prompt-input>
    <app-approval-bar style="margin: auto; display: none" data-approval hidden>
        <span data-approval-message></span>
    </app-approval-bar>
    <app-quiz-bar style="margin: auto; display: none" data-quiz hidden>
        <span slot="question" data-quiz-question></span>
    </app-quiz-bar>
    <div class="error" data-error></div>
</main>
<app-file-manager data-file-manager thread-id="{{thread_id}}"></app-file-manager>
<script type="module">
    document.addEventListener('navigate', (e) => {
        window.location.href = e.detail.path === '/login' ? '/auth/login' : e.detail.path;
    });
</script>"#;

const FILES_TEMPLATE: &str = r#"<style>
    #files-page .brand {
        align-items: center;
        display: flex;
        gap: 10px;
        margin-bottom: 10px;
        padding: 4px;
    }

    #files-page .mark {
        align-items: center;
        background: var(--primary);
        border-radius: 8px;
        color: var(--primary-foreground);
        display: inline-flex;
        font-size: 13px;
        font-weight: 700;
        height: 32px;
        justify-content: center;
        width: 32px;
    }

    #files-page .brand strong {
        color: var(--foreground);
        flex: 1;
        font-size: 14px;
        font-weight: 650;
        min-width: 0;
    }

    #files-page app-sidebar[status="collapsed"] .brand {
        justify-content: center;
        padding: 8px;
    }

    #files-page app-sidebar[status="collapsed"] .brand .mark,
    #files-page app-sidebar[status="collapsed"] .brand strong {
        display: none;
    }

    #files-page app-sidebar[status="collapsed"] [data-sidebar-list] {
        display: none;
    }

    #files-page .thread-label {
        display: block;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    #files-page .sidebar-footer {
        display: flex;
        flex-direction: column;
        gap: 4px;
        padding: 8px;
        width: 100%;
        box-sizing: border-box;
    }

    #files-page .sidebar-action {
        width: 100%;
    }

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

        #files-page .sidebar-brand-toggle {
            display: none;
        }
    }
</style>
{{> sidebar}}
<main>
    <div class="mobile-bar"><app-sidebar-toggle class="mobile-sidebar-toggle"></app-sidebar-toggle></div>
    <app-file-browser></app-file-browser>
</main>
<script type="module">
    document.addEventListener('navigate', (e) => {
        window.location.href = e.detail.path === '/login' ? '/auth/login' : e.detail.path;
    });
</script>"#;

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{get_templates, render_page};

    #[test]
    fn threads_page_matches_showcase_shell_without_extra_layout_wrapper() {
        let hb = get_templates().unwrap();
        let html = render_page(
            &hb,
            "Threads",
            "",
            "threads",
            &json!({
                "thread_id": "thread-1",
                "current_title": "Current thread",
                "running": true,
                "projects": [
                    {
                        "id": "project-1",
                        "title": "My Project",
                        "threads": [{"id": "thread-1", "title": "Current thread", "active": true}]
                    }
                ],
                "ungrouped_threads": [],
                "messages": [
                    {
                        "id": "message-1",
                        "message_type": "agent",
                        "tool_name": "Tool output",
                        "has_thinking": false,
                        "seq": 1,
                        "role": "tool",
                        "content": "done"
                    }
                ]
            }),
        );

        assert!(
            html.contains(
                r#"<body id="threads-page" data-thread-id="thread-1" data-running="true">"#
            )
        );
        assert!(!html.contains(r#"{{> sidebar}}"#));
        assert!(html.contains(r#"<nav>"#));
        assert!(html.contains(r#"<main>"#));
        assert!(html.contains(r#"<header>"#));
        assert!(html.contains(
            r#"<app-sidebar-toggle class="sidebar-brand-toggle" brand="F"></app-sidebar-toggle>"#
        ));
        assert!(html.contains(
            r#"<app-sidebar-toggle class="mobile-sidebar-toggle"></app-sidebar-toggle>"#
        ));
        assert!(html.contains(r#"<section class="content">"#));
        assert!(html.contains(r#"<div class="wrapper" data-messages>"#));
        assert!(html.contains(r#"data-current-title hidden"#));
        assert!(html.contains(r#"tool_names="Tool output""#));
        assert!(!html.contains(r#"tool_name="Tool output""#));
        assert!(!html.contains(r#"<div id="threads-page""#));
        assert!(!html.contains(r#"class="topbar""#));
    }

    #[test]
    fn files_page_renders_browser_and_active_nav() {
        let hb = get_templates().unwrap();
        let html = render_page(
            &hb,
            "Files",
            "",
            "files",
            &json!({
                "projects": [],
                "ungrouped_threads": [],
                "files_active": true,
            }),
        );

        assert!(html.contains(r#"<body id="files-page">"#));
        assert!(html.contains(r#"<app-file-browser></app-file-browser>"#));
        assert!(html.contains(r#"<app-sidebar-nav-item target="/files" active>"#));
        assert!(!html.contains(r#"{{> sidebar}}"#));
    }
}
