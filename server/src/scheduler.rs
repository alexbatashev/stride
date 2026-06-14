//! In-process cron watcher. Runs on a dedicated current-thread runtime because
//! agent and python execution futures are `!Send`. Polls the `automations`
//! table once a minute and fires anything whose schedule matches the current
//! wall-clock minute. Five-minute accuracy is plenty, so a coarse poll is fine.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use friday_agent::{AgentConfig, AgentResponseChunk, BaseAgent, Tool};
use futures::StreamExt;
use minisql::ConnectionPool;
use uuid::Uuid;

use crate::config::Tools;
use crate::cron::Cron;
use crate::db::{AutomationKind, RunStatus, automation_runs, automations};
use crate::runner::inproc::python_tool_config;

const POLL_SECS: u64 = 60;
const AGENT_SYSTEM_PROMPT: &str = "You are Friday, running a scheduled automation with no interactive user. \
     Complete the task and produce a concise final report. You cannot ask questions.";

pub fn spawn(db: ConnectionPool, model_config: Arc<AgentConfig>, tools: Tools) {
    std::thread::Builder::new()
        .name("friday-scheduler".to_string())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("scheduler runtime");
            let local = tokio::task::LocalSet::new();
            local.block_on(&runtime, run(db, model_config, tools));
        })
        .expect("scheduler thread");
}

async fn run(db: ConnectionPool, model_config: Arc<AgentConfig>, tools: Tools) {
    let mut tick = tokio::time::interval(Duration::from_secs(POLL_SECS));
    loop {
        tick.tick().await;
        let now = unix_now();
        let due = match due_automations(&db, now).await {
            Ok(due) => due,
            Err(error) => {
                tracing::warn!(%error, "scheduler failed to load automations");
                continue;
            }
        };
        for automation in due {
            // Mark before dispatch so a slow run is not retriggered next tick.
            if let Err(error) = automations::update()
                .last_run(Some(now))
                .where_(automations::id.eq(automation.id))
                .execute(&db)
                .await
            {
                tracing::warn!(%error, "scheduler failed to mark automation");
                continue;
            }
            let db = db.clone();
            let model_config = model_config.clone();
            let tools = tools.clone();
            tokio::task::spawn_local(async move {
                execute(db, model_config, tools, automation).await;
            });
        }
    }
}

struct DueAutomation {
    id: Uuid,
    kind: AutomationKind,
    payload: String,
}

async fn due_automations(
    db: &ConnectionPool,
    now: i64,
) -> Result<Vec<DueAutomation>, Box<dyn std::error::Error + Send + Sync>> {
    let rows = automations::select()
        .where_(automations::enabled.eq(true))
        .all(db)
        .await?;
    Ok(rows
        .into_iter()
        .filter(|row| {
            let Ok(cron) = Cron::parse(&row.schedule) else {
                tracing::warn!(schedule = %row.schedule, "skipping automation with invalid schedule");
                return false;
            };
            let fresh = row.last_run.is_none_or(|last| last / 60 != now / 60);
            fresh && cron.matches_at(now)
        })
        .map(|row| DueAutomation {
            id: row.id,
            kind: row.kind,
            payload: row.payload,
        })
        .collect())
}

async fn execute(
    db: ConnectionPool,
    model_config: Arc<AgentConfig>,
    tools: Tools,
    automation: DueAutomation,
) {
    let run_id = Uuid::now_v7();
    let started_at = unix_now();
    if let Err(error) = automation_runs::insert()
        .id(run_id)
        .automation_id(automation.id)
        .started_at(started_at)
        .status(RunStatus::Running)
        .output("")
        .execute(&db)
        .await
    {
        tracing::warn!(%error, "scheduler failed to record run start");
        return;
    }

    let result = match automation.kind {
        AutomationKind::Python => run_python(&tools, model_config, &automation.payload).await,
        AutomationKind::Agent => run_agent(model_config, &automation.payload).await,
    };
    let (status, output) = match result {
        Ok(output) => (RunStatus::Success, output),
        Err(output) => (RunStatus::Failed, output),
    };

    if let Err(error) = automation_runs::update()
        .finished_at(Some(unix_now()))
        .status(status)
        .output(output)
        .where_(automation_runs::id.eq(run_id))
        .execute(&db)
        .await
    {
        tracing::warn!(%error, "scheduler failed to record run result");
    }
}

async fn run_python(
    tools: &Tools,
    model_config: Arc<AgentConfig>,
    script: &str,
) -> Result<String, String> {
    let Some(python) = tools.python.as_ref().filter(|p| p.enabled != Some(false)) else {
        return Err("python execution is disabled".to_string());
    };
    let config = python_tool_config(python);
    let dir = config
        .cache_dir
        .join("automations")
        .join(Uuid::now_v7().as_simple().to_string());
    let fs = execenv::DirectOsFileSystem::new(dir).map_err(|e| e.to_string())?;
    let tool = execenv::PythonTool::new(config, Arc::new(fs))
        .await
        .map_err(|e| e.to_string())?;

    let result = tool
        .execute(model_config, serde_json::json!({ "script": script }))
        .await;
    let stdout = result["stdout"].as_str().unwrap_or_default();
    let stderr = result["stderr"].as_str().unwrap_or_default();
    let output = format!("{stdout}{stderr}");
    if result["success"].as_bool().unwrap_or(false) {
        Ok(output)
    } else {
        let error = result["error"]
            .as_str()
            .unwrap_or("python execution failed");
        Err(format!("{output}{error}"))
    }
}

async fn run_agent(model_config: Arc<AgentConfig>, prompt: &str) -> Result<String, String> {
    let agent = BaseAgent::new(
        "default".to_string(),
        model_config,
        AGENT_SYSTEM_PROMPT.to_string(),
        Vec::new(),
    );
    let mut stream = agent.make_turn(prompt.to_string()).await;
    let mut output = String::new();
    while let Some(item) = stream.next().await {
        match item {
            Ok(AgentResponseChunk::Chunk(chunk)) => {
                for choice in &chunk.choices {
                    if let Some(message) = &choice.message {
                        output.push_str(&message.content);
                    }
                }
            }
            Ok(_) => {}
            Err(error) => return Err(format!("{output}\n{error}")),
        }
    }
    Ok(output)
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
