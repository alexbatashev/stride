pub mod agent;
pub mod auth;

use handlebars::Handlebars;
use serde_json::Value;

pub fn get_templates() -> anyhow::Result<Handlebars<'static>> {
    let mut hb = Handlebars::new();
    hb.register_template_string("base", BASE_TEMPLATE)?;
    hb.register_template_string("auth", AUTH_TEMPLATE)?;
    hb.register_template_string("sidebar", SIDEBAR_PARTIAL)?;
    hb.register_template_string("threads", THREADS_TEMPLATE)?;
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
    hb.render(
        "base",
        &serde_json::json!({"title": title, "page_script": page_script, "body": body}),
    )
    .unwrap()
}

const BASE_TEMPLATE: &str = r#"<!doctype html>
<html lang="en">
    <head>
        <meta charset="utf-8" />
        <meta name="viewport" content="width=device-width, initial-scale=1" />
        <title>{{title}}</title>
        <script type="importmap">{"imports": {"lit": "/static/lit.js", "lit/decorators.js": "/static/lit-decorators.js"}}</script>
        <link rel="stylesheet" href="/static/common.css" />
        <script type="module" src="/static/api.js"></script>
        <script type="module" src="/static/components.js"></script>
        <script type="module">
            import { render } from "lit";
            import {
                BOT_MESSAGE_SQUARE,
                WORKFLOW,
            } from "/static/components.js";

            render(
                BOT_MESSAGE_SQUARE,
                document.querySelector("\#new-task-icon"),
            );
            render(WORKFLOW, document.querySelector("\#workflow-icon"));
        </script>
        {{{page_script}}}
    </head>
    <body>
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
        </div>
        <app-sidebar-nav-item target="/threads">
            <span id="new-task-icon" slot="icon"></span>
            New task
        </app-sidebar-nav-item>
        <app-sidebar-nav-item target="/threads">
            <span id="workflow-icon" slot="icon"></span>
            Automations
        </app-sidebar-nav-item>
        <app-sidebar-group title="Threads" data-thread-list>
            {{#each threads}}
                <app-sidebar-group-item target="/threads/{{id}}" {{#if active}}active{{/if}} data-thread-id="{{id}}">
                    <span class="thread-label">{{title}}</span>
                </app-sidebar-group-item>
            {{/each}}
        </app-sidebar-group>
        <app-button slot="footer" class="sidebar-action" variant="secondary" data-action="logout">Log out</app-button>
    </app-sidebar>
</nav>"#;

const THREADS_TEMPLATE: &str = r#"<style>
    #threads-page {
        background: var(--background);
        color: var(--foreground);
        display: flex;
        font-family:
            ui-sans-serif,
            system-ui,
            -apple-system,
            BlinkMacSystemFont,
            "Segoe UI",
            sans-serif;
        height: 100svh;
        overflow: hidden;
        width: 100%;
    }

    #threads-page > nav {
        height: 100svh;
    }

    #threads-page .sidebar-header,
    #threads-page .sidebar-footer {
        padding: 8px;
    }

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
        font-size: 14px;
        font-weight: 650;
    }

    #threads-page .thread-label {
        display: block;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    #threads-page main {
        display: grid;
        flex: 1;
        grid-template-rows: auto 1fr auto;
        height: 100svh;
        min-height: 0;
        min-width: 0;
        overflow: hidden;
    }

    #threads-page .topbar {
        align-items: center;
        backdrop-filter: blur(18px);
        background: var(--topbar-bg);
        border-bottom: 1px solid var(--border);
        display: flex;
        gap: 10px;
        min-height: 52px;
        padding: 0 clamp(14px, 2.4vw, 28px);
        position: sticky;
        top: 0;
        z-index: 10;
    }

    #threads-page .topbar h1 {
        color: var(--card-foreground);
        font-size: 14px;
        font-weight: 600;
        margin: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    #threads-page .messages {
        box-sizing: border-box;
        margin: 0 auto;
        max-width: 100%;
        min-height: 0;
        overflow-y: auto;
        padding: 32px clamp(18px, 4vw, 32px) 24px;
        scrollbar-width: thin;
        width: 100%;
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

    #threads-page .composer-wrap {
        background: var(--surface-gradient);
        bottom: 0;
        padding: 18px clamp(14px, 4vw, 28px) 24px;
        position: sticky;
        z-index: 10;
    }

    #threads-page app-prompt-input {
        margin: 0 auto;
        max-width: 860px;
        width: 100%;
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

    @media (max-width: 760px) {
        #threads-page {
            display: block;
            height: auto;
            min-height: 100svh;
            overflow: visible;
        }

        #threads-page main {
            height: 100svh;
        }

        #threads-page .messages {
            max-width: 100%;
            padding: 8px;
            width: 100%;
        }

        #threads-page .composer-wrap {
            padding: 12px 10px 12px;
        }
    }
</style>
<div id="threads-page" data-thread-id="{{thread_id}}" data-running="{{running}}">
    {{> sidebar}}
    <main>
        <header class="topbar">
            <app-sidebar-toggle></app-sidebar-toggle>
            <h1 data-current-title>{{current_title}}</h1>
        </header>
        <section class="messages" data-messages>
            {{#if messages}}
                {{#each messages}}
                    <app-message
                        message_id="{{id}}"
                        type="{{message_type}}"
                        {{#if tool_name}}tool_name="{{tool_name}}"{{/if}}
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
        </section>
        <footer class="composer-wrap">
            <app-prompt-input
                data-prompt
                placeholder="{{#if thread_id}}Message Friday{{else}}Ask Friday anything{{/if}}"
                {{#if running}}disabled{{/if}}
            ></app-prompt-input>
            <div class="error" data-error></div>
        </footer>
    </main>
</div>
<script type="module">
    document.addEventListener('navigate', (e) => {
        window.location.href = e.detail.path === '/login' ? '/auth/login' : e.detail.path;
    });
</script>"#;
