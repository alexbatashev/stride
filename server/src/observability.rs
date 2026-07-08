use std::{
    collections::HashMap,
    fmt::Write,
    sync::atomic::{AtomicU64, Ordering},
};

use axum::{Router, extract::State, response::IntoResponse, routing::get};
use minisql::ConnectionPool;
use std::sync::{Arc, Mutex};
use stride_agent::{AgentObserver, TokenUsage};

#[derive(Default)]
pub(crate) struct Observability {
    tool_calls: AtomicU64,
    user_messages: AtomicU64,
    agent_messages: AtomicU64,
    input_tokens: AtomicU64,
    output_tokens: AtomicU64,
    model_tokens: Mutex<HashMap<String, TokenTotals>>,
    provider_tokens: Mutex<HashMap<String, TokenTotals>>,
}

#[derive(Clone, Copy, Default)]
struct TokenTotals {
    input: u64,
    output: u64,
}

impl Observability {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub(crate) fn router(self: Arc<Self>, db: ConnectionPool) -> Router {
        Router::new()
            .route("/metrics", get(metrics))
            .with_state(MetricsState {
                db,
                observability: self,
            })
    }

    fn model_token_snapshot(&self) -> Vec<(String, TokenTotals)> {
        token_snapshot(&self.model_tokens)
    }

    fn provider_token_snapshot(&self) -> Vec<(String, TokenTotals)> {
        token_snapshot(&self.provider_tokens)
    }
}

fn token_snapshot(totals: &Mutex<HashMap<String, TokenTotals>>) -> Vec<(String, TokenTotals)> {
    let mut rows: Vec<_> = totals
        .lock()
        .unwrap()
        .iter()
        .map(|(label, totals)| (label.clone(), *totals))
        .collect();
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    rows
}

impl AgentObserver for Observability {
    fn user_message_added(&self) {
        self.user_messages.fetch_add(1, Ordering::Relaxed);
    }
    fn agent_message_started(&self) {
        self.agent_messages.fetch_add(1, Ordering::Relaxed);
    }
    fn tool_call_started(&self, _name: &str) {
        self.tool_calls.fetch_add(1, Ordering::Relaxed);
    }
    fn token_usage(&self, usage: TokenUsage) {
        self.input_tokens
            .fetch_add(usage.input_tokens, Ordering::Relaxed);
        self.output_tokens
            .fetch_add(usage.output_tokens, Ordering::Relaxed);
        record_usage(&self.model_tokens, &usage.model, &usage);
        record_usage(&self.provider_tokens, &usage.provider, &usage);
    }
}

fn record_usage(totals: &Mutex<HashMap<String, TokenTotals>>, label: &str, usage: &TokenUsage) {
    let label = normalized_label(label);
    let mut totals = totals.lock().unwrap();
    let entry = totals.entry(label).or_default();
    entry.input = entry.input.saturating_add(usage.input_tokens);
    entry.output = entry.output.saturating_add(usage.output_tokens);
}

fn normalized_label(label: &str) -> String {
    let label = label.trim();
    if label.is_empty() {
        "unknown".to_string()
    } else {
        label.to_string()
    }
}

#[derive(Clone)]
struct MetricsState {
    db: ConnectionPool,
    observability: Arc<Observability>,
}

async fn metrics(State(state): State<MetricsState>) -> impl IntoResponse {
    let snapshot = DbSnapshot::load(&state.db).await.unwrap_or_default();
    let obs = state.observability;
    let mut out = String::new();
    metric(
        &mut out,
        "stride_threads_total",
        "Total threads stored by Stride.",
        snapshot.threads,
    );
    metric(
        &mut out,
        "stride_messages_user_total",
        "User messages stored by Stride.",
        snapshot.user_messages,
    );
    metric(
        &mut out,
        "stride_messages_agent_total",
        "Agent messages stored by Stride.",
        snapshot.agent_messages,
    );
    metric(
        &mut out,
        "stride_tool_messages_total",
        "Tool result messages stored by Stride.",
        snapshot.tool_messages,
    );
    metric(
        &mut out,
        "stride_process_tool_calls_total",
        "Tool calls observed since this server process started.",
        obs.tool_calls.load(Ordering::Relaxed),
    );
    metric(
        &mut out,
        "stride_process_user_messages_total",
        "User messages observed since this server process started.",
        obs.user_messages.load(Ordering::Relaxed),
    );
    metric(
        &mut out,
        "stride_process_agent_messages_total",
        "Agent message turns observed since this server process started.",
        obs.agent_messages.load(Ordering::Relaxed),
    );
    metric(
        &mut out,
        "stride_process_input_tokens_total",
        "Input tokens reported by model providers since this server process started.",
        obs.input_tokens.load(Ordering::Relaxed),
    );
    metric(
        &mut out,
        "stride_process_output_tokens_total",
        "Output tokens reported by model providers since this server process started.",
        obs.output_tokens.load(Ordering::Relaxed),
    );
    let model_tokens = obs.model_token_snapshot();
    let provider_tokens = obs.provider_token_snapshot();
    labeled_token_metric(
        &mut out,
        "stride_process_model_input_tokens_total",
        "Input tokens reported by model since this server process started.",
        "model",
        &model_tokens,
        |totals| totals.input,
    );
    labeled_token_metric(
        &mut out,
        "stride_process_model_output_tokens_total",
        "Output tokens reported by model since this server process started.",
        "model",
        &model_tokens,
        |totals| totals.output,
    );
    labeled_token_metric(
        &mut out,
        "stride_process_model_tokens_total",
        "Total tokens reported by model since this server process started.",
        "model",
        &model_tokens,
        |totals| totals.input.saturating_add(totals.output),
    );
    labeled_token_metric(
        &mut out,
        "stride_process_provider_input_tokens_total",
        "Input tokens reported by provider since this server process started.",
        "provider",
        &provider_tokens,
        |totals| totals.input,
    );
    labeled_token_metric(
        &mut out,
        "stride_process_provider_output_tokens_total",
        "Output tokens reported by provider since this server process started.",
        "provider",
        &provider_tokens,
        |totals| totals.output,
    );
    labeled_token_metric(
        &mut out,
        "stride_process_provider_tokens_total",
        "Total tokens reported by provider since this server process started.",
        "provider",
        &provider_tokens,
        |totals| totals.input.saturating_add(totals.output),
    );
    (
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        out,
    )
}

fn metric(out: &mut String, name: &str, help: &str, value: u64) {
    let _ = writeln!(out, "# HELP {name} {help}");
    let _ = writeln!(out, "# TYPE {name} counter");
    let _ = writeln!(out, "{name} {value}");
}

fn labeled_token_metric(
    out: &mut String,
    name: &str,
    help: &str,
    label_name: &str,
    rows: &[(String, TokenTotals)],
    value: impl Fn(TokenTotals) -> u64,
) {
    let _ = writeln!(out, "# HELP {name} {help}");
    let _ = writeln!(out, "# TYPE {name} counter");
    for (label, totals) in rows {
        let label = prometheus_label_value(label);
        let _ = writeln!(
            out,
            r#"{name}{{{label_name}="{label}"}} {}"#,
            value(*totals)
        );
    }
}

fn prometheus_label_value(value: &str) -> String {
    value
        .replace('\\', r"\\")
        .replace('\n', r"\n")
        .replace('"', r#"\""#)
}

#[derive(Default)]
struct DbSnapshot {
    threads: u64,
    user_messages: u64,
    agent_messages: u64,
    tool_messages: u64,
}

impl DbSnapshot {
    async fn load(db: &ConnectionPool) -> Result<Self, String> {
        Ok(Self {
            threads: count(db, "SELECT COUNT(*) AS count FROM threads").await?,
            user_messages: count(
                db,
                "SELECT COUNT(*) AS count FROM messages WHERE role = 'user'",
            )
            .await?,
            agent_messages: count(
                db,
                "SELECT COUNT(*) AS count FROM messages WHERE role = 'agent'",
            )
            .await?,
            tool_messages: count(
                db,
                "SELECT COUNT(*) AS count FROM messages WHERE role = 'tool'",
            )
            .await?,
        })
    }
}

async fn count(db: &ConnectionPool, sql: &str) -> Result<u64, String> {
    let result = db
        .query_with_params(sql, vec![])
        .await
        .map_err(|error| error.to_string())?;
    Ok(result
        .rows()
        .first()
        .and_then(|row| row.get_int("count"))
        .unwrap_or_default() as u64)
}
