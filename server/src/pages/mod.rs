pub mod agent;
pub mod archived;
pub mod auth;
pub mod automations;
pub mod files;
pub mod settings;

use crate::api::threads::ThreadPageData;
use crate::components::{
    auth_form::AuthForm,
    shell_page_view::{
        RenderStores as ShellRenderStores, ShellPageData, ShellPageView, ShellPageViewServer,
    },
    threads_page_view::{DocumentOpts as ArgonDocumentOpts, ThreadPageData as ArgonThreadPageData},
    timeline::TimelineMessage,
    ui::Stores as UiStores,
};

fn argon_thread_page_data(data: ThreadPageData) -> ArgonThreadPageData {
    ArgonThreadPageData {
        thread_id: data.thread_id,
        current_title: data.current_title,
        selected_model: data.selected_model,
        running: data.running,
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
        messages: data
            .messages
            .into_iter()
            .map(|message| TimelineMessage {
                id: message.id,
                seq: message.seq as f64,
                role: message.role.to_string(),
                message_type: message.message_type.to_string(),
                format: message.format.to_string(),
                content: message.content,
                thinking: message.thinking.unwrap_or_default(),
                tool_name: message.tool_name.unwrap_or_default(),
            })
            .collect(),
    }
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

    #[tokio::test]
    async fn threads_page_renders_argon_server_data_and_shared_timeline() {
        let data = super::argon_thread_page_data(sample_data());
        let server = TestPageServer(data);
        let ui = UiStores::default();
        let thread_view = crate::components::thread_view::Stores::default();
        let stores = RenderStores {
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
        assert!(html.contains(r#"data-message-id="message-1""#));
        assert!(html.contains(r#"data-kind="tool_output""#));
        assert!(html.contains(r#"data-message-id="message-2""#));
        assert!(html.contains("hello &amp; &lt;world&gt;"));
        assert!(html.contains(r#"data-message-id="message-3""#));
        assert!(html.contains("Here's Research &amp; Web"));
        assert!(html.contains("A &amp; B"));
        assert!(html.contains(r#"data-running="true""#));
        assert!(html.contains(r#"data-prompt"#));
        assert!(html.contains(r#"data-approval hidden"#));
        assert!(html.contains(r#"data-quiz hidden"#));
        assert!(html.contains(r#"<app-file-manager data-file-manager data-thread-id="thread-1""#));
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
