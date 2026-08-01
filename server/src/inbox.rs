//! The inbox: actions the agent proposed on its own initiative and parked for
//! the user to approve or decline. A suggestion stores everything needed to
//! carry the action out later, so approving it is a single click and never
//! needs the originating conversation to still be alive.

use minisql::ConnectionPool;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    cron::Cron,
    db::{
        AutomationKind, InboxKind, InboxStatus, NotifyKind, TriggerKind, automations, inbox_items,
        projects, threads,
    },
    runner::AgentRequest,
    triggers::webhook,
    user_events::{self, UserEventKind},
};

/// What a suggestion does once approved. Serialized into `inbox_items.payload`.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum InboxAction {
    FollowUp(FollowUp),
    Automation(Automation),
    Skill(Skill),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct FollowUp {
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_title: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct Automation {
    pub name: String,
    #[serde(default)]
    pub schedule: String,
    pub kind: String,
    pub task: String,
    #[serde(default = "default_trigger")]
    pub trigger: String,
    #[serde(default = "default_notify")]
    pub notify: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub content: String,
}

fn default_trigger() -> String {
    "cron".to_string()
}

fn default_notify() -> String {
    "none".to_string()
}

impl InboxAction {
    pub fn kind(&self) -> InboxKind {
        match self {
            InboxAction::FollowUp(_) => InboxKind::FollowUp,
            InboxAction::Automation(_) => InboxKind::Automation,
            InboxAction::Skill(_) => InboxKind::Skill,
        }
    }

    /// Reject a suggestion the user could never act on before it reaches their
    /// inbox, so bad cron expressions and empty prompts fail at the tool call
    /// instead of at approval time.
    pub fn validate(&self) -> Result<(), String> {
        match self {
            InboxAction::FollowUp(follow_up) => {
                if follow_up.message.trim().is_empty() {
                    return Err("message must not be empty".to_string());
                }
            }
            InboxAction::Automation(automation) => {
                if automation.name.trim().is_empty() {
                    return Err("name must not be empty".to_string());
                }
                if automation.task.trim().is_empty() {
                    return Err("task must not be empty".to_string());
                }
                automation_kind(&automation.kind)?;
                let trigger = trigger_kind(&automation.trigger)?;
                notify_kind(&automation.notify)?;
                if trigger == TriggerKind::Cron && Cron::parse(automation.schedule.trim()).is_err()
                {
                    return Err("invalid cron schedule".to_string());
                }
            }
            InboxAction::Skill(skill) => {
                if skill.name.trim().is_empty() {
                    return Err("name must not be empty".to_string());
                }
                if skill.description.trim().is_empty() {
                    return Err("description must not be empty".to_string());
                }
                if skill.content.trim().is_empty() {
                    return Err("content must not be empty".to_string());
                }
            }
        }
        Ok(())
    }
}

fn automation_kind(value: &str) -> Result<AutomationKind, String> {
    match value {
        "python" => Ok(AutomationKind::Python),
        "agent" => Ok(AutomationKind::Agent),
        other => Err(format!("invalid automation kind: {other}")),
    }
}

fn trigger_kind(value: &str) -> Result<TriggerKind, String> {
    match value {
        "cron" => Ok(TriggerKind::Cron),
        "webhook" => Ok(TriggerKind::Webhook),
        "manual" => Ok(TriggerKind::Manual),
        other => Err(format!("invalid trigger: {other}")),
    }
}

fn notify_kind(value: &str) -> Result<NotifyKind, String> {
    match value {
        "none" => Ok(NotifyKind::None),
        "telegram" => Ok(NotifyKind::Telegram),
        other => Err(format!("invalid notify target: {other}")),
    }
}

/// What approving a suggestion produced, stored in `inbox_items.outcome`.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub struct Outcome {
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub href: Option<String>,
}

pub struct NewSuggestion {
    pub owner: Uuid,
    pub thread_id: Option<Uuid>,
    pub title: String,
    pub rationale: String,
    pub action: InboxAction,
}

/// Park a suggestion in the owner's inbox and tell their open tabs about it.
pub async fn suggest(
    db: &ConnectionPool,
    clock: &dyn stride_agent::Clock,
    id_gen: &dyn stride_agent::IdGen,
    suggestion: NewSuggestion,
) -> Result<Uuid, String> {
    suggestion.action.validate()?;
    let payload = serde_json::to_string(&suggestion.action).map_err(|e| e.to_string())?;
    let id = id_gen.new_uuid_v7();

    inbox_items::insert()
        .id(id)
        .owner(suggestion.owner)
        .thread_id(suggestion.thread_id)
        .kind(suggestion.action.kind())
        .title(suggestion.title.trim())
        .rationale(suggestion.rationale.trim())
        .payload(payload.as_str())
        .status(InboxStatus::Pending)
        .created_at(clock.now_unix_secs())
        .execute(db)
        .await
        .map_err(|e| e.to_string())?;

    publish_pending_count(db, id_gen, suggestion.owner).await;
    Ok(id)
}

/// Broadcast the owner's pending count so every open tab can refresh its badge.
pub async fn publish_pending_count(
    db: &ConnectionPool,
    id_gen: &dyn stride_agent::IdGen,
    owner: Uuid,
) {
    let Ok(pending) = pending_count(db, owner).await else {
        return;
    };
    user_events::publish(
        owner,
        id_gen.new_uuid_v7(),
        UserEventKind::InboxUpdated { pending },
    );
}

pub async fn pending_count(db: &ConnectionPool, owner: Uuid) -> Result<u64, String> {
    let rows = inbox_items::select_cols((inbox_items::id,))
        .where_(
            inbox_items::owner
                .eq(owner)
                .and(inbox_items::status.eq(InboxStatus::Pending)),
        )
        .all(db)
        .await
        .map_err(|e| e.to_string())?;
    Ok(rows.len() as u64)
}

/// Everything carrying out an approved action needs. Kept separate from
/// `ServerState` so the logic stays testable without a full server.
pub struct ApprovalContext<'a> {
    pub db: &'a ConnectionPool,
    pub clock: &'a dyn stride_agent::Clock,
    pub id_gen: &'a dyn stride_agent::IdGen,
    pub skills: &'a crate::skills::SkillStore,
    pub runner: &'a dyn crate::runner::AgentPool,
}

/// Carry out an approved action. Returns what happened so the caller can store
/// it on the row and show it in the UI.
pub async fn execute(
    ctx: &ApprovalContext<'_>,
    owner: Uuid,
    action: &InboxAction,
) -> Result<Outcome, String> {
    action.validate()?;
    match action {
        InboxAction::FollowUp(follow_up) => start_follow_up_thread(ctx, owner, follow_up).await,
        InboxAction::Automation(automation) => create_automation(ctx, owner, automation).await,
        InboxAction::Skill(skill) => create_skill(ctx, owner, skill).await,
    }
}

async fn start_follow_up_thread(
    ctx: &ApprovalContext<'_>,
    owner: Uuid,
    follow_up: &FollowUp,
) -> Result<Outcome, String> {
    let project_id = match follow_up.project.as_deref().map(str::trim) {
        Some(reference) if !reference.is_empty() => {
            Some(resolve_project(ctx.db, owner, reference).await?)
        }
        _ => None,
    };
    let title = follow_up
        .thread_title
        .as_deref()
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| derive_title(&follow_up.message));

    let thread_id = ctx.id_gen.new_uuid_v7();
    let mut insert = threads::insert()
        .id(thread_id)
        .owner(owner)
        .title(title.as_str())
        .last_activity_at(Some(ctx.clock.now_unix_millis()));
    if let Some(project_id) = project_id {
        insert = insert.project_id(project_id);
    }
    insert.execute(ctx.db).await.map_err(|e| e.to_string())?;

    user_events::publish(
        owner,
        ctx.id_gen.new_uuid_v7(),
        UserEventKind::ThreadCreated {
            thread_id,
            title: title.clone(),
            project_id,
        },
    );

    ctx.runner
        .send(
            thread_id,
            AgentRequest {
                content: follow_up.message.clone(),
                images: Vec::new(),
                model: None,
            },
        )
        .await
        .map_err(|e| e.to_string())?;

    Ok(Outcome {
        summary: format!("Started thread “{title}”"),
        href: Some(format!("/threads/{thread_id}")),
    })
}

async fn create_automation(
    ctx: &ApprovalContext<'_>,
    owner: Uuid,
    automation: &Automation,
) -> Result<Outcome, String> {
    let kind = automation_kind(&automation.kind)?;
    let trigger = trigger_kind(&automation.trigger)?;
    let notify = notify_kind(&automation.notify)?;
    let webhook_secret = match trigger {
        TriggerKind::Webhook => Some(webhook::generate_secret()),
        _ => None,
    };
    let id = ctx.id_gen.new_uuid_v7();

    automations::insert()
        .id(id)
        .owner(owner)
        .name(automation.name.trim())
        .schedule(automation.schedule.trim())
        .kind(kind)
        .payload(automation.task.as_str())
        .enabled(true)
        .created_at(ctx.clock.now_unix_secs())
        .trigger_kind(Some(trigger.as_str()))
        .webhook_secret(webhook_secret.as_deref())
        .notify_kind(Some(notify.as_str()))
        .execute(ctx.db)
        .await
        .map_err(|e| e.to_string())?;

    Ok(Outcome {
        summary: format!("Created automation “{}”", automation.name.trim()),
        href: Some("/automations".to_string()),
    })
}

async fn create_skill(
    ctx: &ApprovalContext<'_>,
    owner: Uuid,
    skill: &Skill,
) -> Result<Outcome, String> {
    ctx.skills
        .create(
            owner,
            skill.name.trim(),
            skill.description.trim(),
            &skill.content,
        )
        .await
        .map_err(|e| e.to_string())?;

    Ok(Outcome {
        summary: format!("Created skill “{}”", skill.name.trim()),
        href: None,
    })
}

async fn resolve_project(
    db: &ConnectionPool,
    owner: Uuid,
    reference: &str,
) -> Result<Uuid, String> {
    let rows = projects::select()
        .where_(projects::owner.eq(owner))
        .all(db)
        .await
        .map_err(|e| e.to_string())?;

    if let Ok(id) = Uuid::parse_str(reference)
        && rows.iter().any(|row| row.id == id)
    {
        return Ok(id);
    }

    rows.into_iter()
        .find(|row| row.title == reference)
        .map(|row| row.id)
        .ok_or_else(|| format!("project not found: {reference}"))
}

/// Longest thread title derived from a follow-up message.
const MAX_TITLE_LEN: usize = 128;

fn derive_title(message: &str) -> String {
    let title: String = message
        .split_whitespace()
        .take(8)
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(MAX_TITLE_LEN)
        .collect();
    if title.is_empty() {
        crate::api::threads::DEFAULT_THREAD_TITLE.to_string()
    } else {
        title
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use minisql::Value;
    use stride_agent::{AgentConfig, ModelRegistry};

    use super::*;
    use crate::{
        db::{self, automations},
        runner::pool::InProcessAgentPool,
        skills::SkillStore,
        vfs::Vfs,
    };

    async fn setup() -> (ConnectionPool, Uuid, Arc<Vfs>) {
        let db = ConnectionPool::new("sqlite::memory:").unwrap();
        db.initialize_database(db::get_migrations()).await.unwrap();
        let owner = Uuid::now_v7();
        db.query_with_params(
            "INSERT INTO users (id, username, password_hash) VALUES (?, ?, ?)",
            vec![
                Value::Uuid(owner),
                Value::Text("alice".to_string()),
                Value::Text("hash".to_string()),
            ],
        )
        .await
        .unwrap();

        let base = tempfile::tempdir().unwrap().keep();
        let storage = crate::vfs::AnyFileProvider::Local(
            crate::vfs::LocalFileProvider::with_id_gen(base, Arc::new(stride_agent::SystemIdGen))
                .unwrap(),
        );
        let vfs = Arc::new(Vfs::with_clock(
            db.clone(),
            storage,
            3,
            Arc::new(stride_agent::SystemClock),
            Arc::new(stride_agent::SystemIdGen),
        ));
        (db, owner, vfs)
    }

    fn agent_pool(db: &ConnectionPool) -> Arc<InProcessAgentPool> {
        let model_config = Arc::new(AgentConfig {
            model_registry: ModelRegistry::new(),
            max_iterations: 1,
            usage_observer: Arc::new(stride_agent::NoopUsageObserver),
            ..Default::default()
        });
        let config = crate::config::Config {
            providers: Default::default(),
            models: Default::default(),
            server: None,
            tools: None,
            mcp: Default::default(),
        };
        Arc::new(InProcessAgentPool::new(
            db.clone(),
            model_config,
            config,
            crate::crypto::SecretCipher::new("test-secret"),
        ))
    }

    #[tokio::test]
    async fn suggesting_parks_the_action_and_counts_it_as_pending() {
        let (db, owner, _vfs) = setup().await;
        let clock = stride_agent::SystemClock;
        let id_gen = stride_agent::SystemIdGen;

        let id = suggest(
            &db,
            &clock,
            &id_gen,
            NewSuggestion {
                owner,
                thread_id: None,
                title: "  Track vendor pricing  ".to_string(),
                rationale: "You compared vendors twice this week".to_string(),
                action: automation("0 9 * * 1", "cron"),
            },
        )
        .await
        .unwrap();

        let rows = inbox_items::select()
            .where_(inbox_items::id.eq(id))
            .all(&db)
            .await
            .unwrap();
        let row = rows.first().unwrap();
        assert_eq!(row.title, "Track vendor pricing");
        assert_eq!(row.status, InboxStatus::Pending);
        assert_eq!(row.kind, InboxKind::Automation);
        assert_eq!(row.resolved_at, None);
        assert_eq!(pending_count(&db, owner).await.unwrap(), 1);
    }

    #[tokio::test]
    async fn an_invalid_action_never_reaches_the_inbox() {
        let (db, owner, _vfs) = setup().await;
        let error = suggest(
            &db,
            &stride_agent::SystemClock,
            &stride_agent::SystemIdGen,
            NewSuggestion {
                owner,
                thread_id: None,
                title: "Broken".to_string(),
                rationale: String::new(),
                action: automation("every other tuesday", "cron"),
            },
        )
        .await
        .unwrap_err();

        assert_eq!(error, "invalid cron schedule");
        assert_eq!(pending_count(&db, owner).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn approving_an_automation_creates_it() {
        let (db, owner, _vfs) = setup().await;
        let pool = agent_pool(&db);
        let skills = SkillStore::new(None).unwrap();
        let clock = stride_agent::SystemClock;
        let id_gen = stride_agent::SystemIdGen;
        let ctx = ApprovalContext {
            db: &db,
            clock: &clock,
            id_gen: &id_gen,
            skills: &skills,
            runner: pool.as_ref(),
        };

        let outcome = execute(&ctx, owner, &automation("0 9 * * 1", "cron"))
            .await
            .unwrap();
        assert_eq!(outcome.summary, "Created automation “Weekly digest”");
        assert_eq!(outcome.href.as_deref(), Some("/automations"));

        let rows = automations::select()
            .where_(automations::owner.eq(owner))
            .all(&db)
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "Weekly digest");
        assert_eq!(rows[0].schedule, "0 9 * * 1");
        assert!(rows[0].enabled);
    }

    #[tokio::test]
    async fn approving_a_skill_writes_it_to_the_users_skill_store() {
        let (db, owner, vfs) = setup().await;
        let pool = agent_pool(&db);
        let skills = SkillStore::new(Some(vfs)).unwrap();
        let clock = stride_agent::SystemClock;
        let id_gen = stride_agent::SystemIdGen;
        let ctx = ApprovalContext {
            db: &db,
            clock: &clock,
            id_gen: &id_gen,
            skills: &skills,
            runner: pool.as_ref(),
        };

        let action = InboxAction::Skill(Skill {
            name: "vendor-review".to_string(),
            description: "How to compare vendor quotes".to_string(),
            content: "# Vendor review\n\nCompare on price and support.\n".to_string(),
        });
        let outcome = execute(&ctx, owner, &action).await.unwrap();
        assert_eq!(outcome.summary, "Created skill “vendor-review”");

        let stored = skills.user_views(owner).await.unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].name, "vendor-review");
    }

    fn automation(schedule: &str, trigger: &str) -> InboxAction {
        InboxAction::Automation(Automation {
            name: "Weekly digest".to_string(),
            schedule: schedule.to_string(),
            kind: "agent".to_string(),
            task: "Summarize the week".to_string(),
            trigger: trigger.to_string(),
            notify: "none".to_string(),
        })
    }

    #[test]
    fn payload_round_trips_through_json() {
        let action = automation("0 9 * * 1", "cron");
        let encoded = serde_json::to_string(&action).unwrap();
        assert_eq!(
            serde_json::from_str::<InboxAction>(&encoded).unwrap(),
            action
        );
        assert_eq!(action.kind(), InboxKind::Automation);
    }

    #[test]
    fn cron_suggestions_need_a_valid_schedule() {
        assert!(automation("0 9 * * 1", "cron").validate().is_ok());
        assert!(automation("not a cron", "cron").validate().is_err());
        // Non-cron triggers ignore the schedule entirely.
        assert!(automation("", "manual").validate().is_ok());
    }

    #[test]
    fn empty_follow_up_messages_are_rejected() {
        let empty = InboxAction::FollowUp(FollowUp {
            message: "  ".to_string(),
            project: None,
            thread_title: None,
        });
        assert!(empty.validate().is_err());
    }

    #[test]
    fn follow_up_titles_fall_back_to_the_message() {
        assert_eq!(
            derive_title("Draft the launch plan for the new pricing page next week"),
            "Draft the launch plan for the new pricing"
        );
        assert_eq!(
            derive_title("   "),
            crate::api::threads::DEFAULT_THREAD_TITLE
        );
    }
}
