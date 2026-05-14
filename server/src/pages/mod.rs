pub mod agent;
pub mod auth;

use handlebars::Handlebars;
use serde_json::Value;

pub fn get_templates() -> anyhow::Result<Handlebars<'static>> {
    let mut hb = Handlebars::new();
    hb.register_template_string("base", BASE_TEMPLATE)?;
    hb.register_template_string("auth", AUTH_TEMPLATE)?;
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
        <script type="importmap">{"imports": {"lit": "/static/lit.js"}}</script>
        <link rel="stylesheet" href="/static/common.css" />
        <script type="module" src="/static/api.js"></script>
        <script type="module" src="/static/components.js"></script>
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

const THREADS_TEMPLATE: &str = r#"<threads-page thread-id="{{thread_id}}"></threads-page>
<script type="module">
    document.addEventListener('navigate', (e) => {
        window.location.href = e.detail.path === '/login' ? '/auth/login' : e.detail.path;
    });
</script>"#;
