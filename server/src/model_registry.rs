use std::collections::{HashMap, HashSet};

use llm::{Anthropic, Ollama, OpenAI, ReasoningEffort};
use minisql::ConnectionPool;
use stride_agent::{
    DEFAULT_MODEL, EMBEDDING_MODEL, ModelRegEntry, ModelRegistry, TRANSCRIPTION_MODEL,
};
use uuid::Uuid;

use crate::{
    config::{Config, Kind},
    crypto::SecretCipher,
    db::{agent_settings, user_models, user_providers},
};

pub const TITLE_GENERATOR_MODEL: &str = "title_generator";

const INTERNAL_MODEL_KEYS: &[&str] = &[
    EMBEDDING_MODEL,
    TRANSCRIPTION_MODEL,
    TITLE_GENERATOR_MODEL,
    "expert",
    "explorer",
];

#[derive(Clone, Debug, serde::Serialize)]
pub struct ModelSummary {
    pub key: String,
    pub slug: String,
    pub display_name: String,
    pub description: String,
    pub source: &'static str,
    pub provider: String,
    pub vision: bool,
    pub reasoning_effort: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct ProviderSummary {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub url: String,
    pub created_at: i64,
}

#[derive(Clone, Debug, serde::Deserialize)]
pub struct ProviderInput {
    pub name: String,
    pub kind: String,
    pub url: String,
    pub token: String,
}

#[derive(Clone, Debug, serde::Deserialize)]
pub struct UserModelInput {
    pub name: String,
    pub slug: String,
    pub provider_id: Uuid,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub reasoning_effort: Option<String>,
    pub vision: Option<bool>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct UserModelSummary {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub display_name: String,
    pub description: String,
    pub provider_id: String,
    pub provider_name: String,
    pub vision: bool,
    pub reasoning_effort: Option<String>,
    pub created_at: i64,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct AgentSettings {
    #[serde(default)]
    pub subagent_allowed_models: Vec<String>,
    #[serde(default)]
    pub subagent_guidelines: String,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct AgentSettingsResponse {
    #[serde(flatten)]
    pub settings: AgentSettings,
    pub using_server_defaults: bool,
    pub server_default_guidelines: String,
}

pub fn default_subagent_guidelines(config: &Config) -> String {
    config
        .server
        .as_ref()
        .and_then(|server| server.agent.as_ref())
        .and_then(|agent| agent.default_subagent_guidelines.as_deref())
        .unwrap_or("")
        .trim()
        .to_string()
}

pub fn is_chat_model(key: &str) -> bool {
    !INTERNAL_MODEL_KEYS.contains(&key)
}

pub fn config_chat_models(config: &Config) -> Vec<ModelSummary> {
    config
        .models
        .iter()
        .filter(|(key, _)| is_chat_model(key))
        .map(|(key, model)| ModelSummary {
            key: key.clone(),
            slug: model.slug.clone(),
            display_name: model_display_name(key, model.display_name.as_deref()),
            description: optional_text(model.description.as_deref()),
            source: "config",
            provider: model.provider.clone(),
            vision: model.vision.unwrap_or(false),
            reasoning_effort: model.reasoning_effort().map(reasoning_effort_name),
        })
        .collect()
}

pub async fn list_available_models(
    config: &Config,
    db: &ConnectionPool,
    owner: Uuid,
) -> anyhow::Result<Vec<ModelSummary>> {
    let mut models = config_chat_models(config);
    models.extend(list_user_model_summaries(db, owner).await?);
    models.sort_by(|a, b| a.key.cmp(&b.key));
    Ok(models)
}

pub async fn list_user_model_summaries(
    db: &ConnectionPool,
    owner: Uuid,
) -> anyhow::Result<Vec<ModelSummary>> {
    let rows = user_models::select()
        .where_(user_models::owner.eq(owner))
        .order_by_asc(user_models::name)
        .all(db)
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;

    let provider_names = provider_name_map(db, owner).await?;

    Ok(rows
        .into_iter()
        .map(|row| ModelSummary {
            key: row.name.clone(),
            slug: row.slug.clone(),
            display_name: model_display_name(&row.name, row.display_name.as_deref()),
            description: optional_text(row.description.as_deref()),
            source: "user",
            provider: provider_names
                .get(&row.provider_id)
                .cloned()
                .unwrap_or_else(|| "unknown".to_string()),
            vision: row.vision,
            reasoning_effort: row.reasoning_effort.clone(),
        })
        .collect())
}

pub async fn build_user_registry(
    _config: &Config,
    base: &ModelRegistry,
    db: &ConnectionPool,
    owner: Uuid,
    cipher: &SecretCipher,
) -> anyhow::Result<ModelRegistry> {
    let mut registry = base.clone();

    let providers = user_providers::select()
        .where_(user_providers::owner.eq(owner))
        .all(db)
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;

    let provider_map: HashMap<Uuid, _> = providers
        .into_iter()
        .filter_map(|provider| {
            let token = cipher
                .decrypt(provider.id, &provider.token_ciphertext)
                .ok()?;
            Some((provider.id, (provider, token)))
        })
        .collect();

    let models = user_models::select()
        .where_(user_models::owner.eq(owner))
        .all(db)
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;

    for model in models {
        let Some((provider, token)) = provider_map.get(&model.provider_id) else {
            continue;
        };
        let Some(kind) = parse_kind(&provider.kind) else {
            continue;
        };
        registry.add_model(
            &model.name,
            entry_from_provider(
                kind,
                &provider.url,
                token,
                &model.slug,
                &model.reasoning_effort,
                model.vision,
            ),
        );
    }

    Ok(registry)
}

pub async fn load_agent_settings(
    config: &Config,
    db: &ConnectionPool,
    owner: Uuid,
) -> anyhow::Result<AgentSettings> {
    let rows = agent_settings::select()
        .where_(agent_settings::owner.eq(owner))
        .all(db)
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;

    let available = list_available_models(config, db, owner).await?;
    let available_keys: HashSet<_> = available.iter().map(|model| model.key.clone()).collect();
    let default_guidelines = default_subagent_guidelines(config);

    Ok(match rows.into_iter().next() {
        Some(row) => {
            let allowed: Vec<String> = row
                .subagent_allowed_models
                .as_deref()
                .and_then(parse_json_array)
                .unwrap_or_default()
                .into_iter()
                .filter(|key| available_keys.contains(key))
                .collect();
            AgentSettings {
                subagent_allowed_models: if allowed.is_empty() {
                    available.iter().map(|model| model.key.clone()).collect()
                } else {
                    allowed
                },
                subagent_guidelines: row.subagent_guidelines.unwrap_or_default(),
            }
        }
        None => AgentSettings {
            subagent_allowed_models: available.iter().map(|model| model.key.clone()).collect(),
            subagent_guidelines: default_guidelines,
        },
    })
}

pub async fn load_agent_settings_response(
    config: &Config,
    db: &ConnectionPool,
    owner: Uuid,
) -> anyhow::Result<AgentSettingsResponse> {
    let using_server_defaults = !has_custom_agent_settings(db, owner).await?;
    let settings = load_agent_settings(config, db, owner).await?;
    Ok(AgentSettingsResponse {
        settings,
        using_server_defaults,
        server_default_guidelines: default_subagent_guidelines(config),
    })
}

async fn has_custom_agent_settings(db: &ConnectionPool, owner: Uuid) -> anyhow::Result<bool> {
    let rows = agent_settings::select_cols((agent_settings::owner,))
        .where_(agent_settings::owner.eq(owner))
        .all(db)
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    Ok(!rows.is_empty())
}

pub async fn save_agent_settings(
    db: &ConnectionPool,
    owner: Uuid,
    settings: &AgentSettings,
) -> anyhow::Result<()> {
    let allowed_json = serde_json::to_string(&settings.subagent_allowed_models)
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    let guidelines = settings.subagent_guidelines.trim();
    db.query_with_params(
        "INSERT INTO agent_settings (owner, subagent_allowed_models, subagent_guidelines, updated_at) \
         VALUES (?, ?, ?, ?) \
         ON CONFLICT(owner) DO UPDATE SET \
            subagent_allowed_models = excluded.subagent_allowed_models, \
            subagent_guidelines = excluded.subagent_guidelines, \
            updated_at = excluded.updated_at",
        vec![
            minisql::Value::Uuid(owner),
            minisql::Value::Text(allowed_json),
            if guidelines.is_empty() {
                minisql::Value::Null
            } else {
                minisql::Value::Text(guidelines.to_string())
            },
            minisql::Value::Integer(now_secs()),
        ],
    )
    .await
    .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    Ok(())
}

pub async fn list_providers(
    db: &ConnectionPool,
    owner: Uuid,
) -> anyhow::Result<Vec<ProviderSummary>> {
    let rows = user_providers::select()
        .where_(user_providers::owner.eq(owner))
        .order_by_asc(user_providers::name)
        .all(db)
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;

    Ok(rows
        .into_iter()
        .map(|row| ProviderSummary {
            id: row.id.to_string(),
            name: row.name,
            kind: row.kind,
            url: row.url,
            created_at: row.created_at,
        })
        .collect())
}

pub async fn create_provider(
    db: &ConnectionPool,
    cipher: &SecretCipher,
    owner: Uuid,
    input: ProviderInput,
) -> anyhow::Result<ProviderSummary> {
    let name = normalize_name(&input.name)?;
    let kind = normalize_kind(&input.kind)?;
    let url = normalize_url(&input.url)?;
    let token = input.token.trim();
    if token.is_empty() {
        anyhow::bail!("token is required");
    }

    ensure_unique_provider_name(db, owner, &name).await?;

    let id = Uuid::now_v7();
    let created_at = now_secs();
    let token_ciphertext = cipher
        .encrypt(id, token)
        .map_err(|error| anyhow::anyhow!(error))?;

    user_providers::insert()
        .id(id)
        .owner(owner)
        .name(name.as_str())
        .kind(kind.as_str())
        .url(url.as_str())
        .token_ciphertext(token_ciphertext.as_str())
        .created_at(created_at)
        .execute(db)
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;

    Ok(ProviderSummary {
        id: id.to_string(),
        name,
        kind,
        url,
        created_at,
    })
}

pub async fn delete_provider(db: &ConnectionPool, owner: Uuid, id: Uuid) -> anyhow::Result<()> {
    let existing = user_providers::select_cols((user_providers::id,))
        .where_(
            user_providers::id
                .eq(id)
                .and(user_providers::owner.eq(owner)),
        )
        .all(db)
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    if existing.is_empty() {
        anyhow::bail!("provider not found");
    }

    user_models::delete()
        .where_(
            user_models::provider_id
                .eq(id)
                .and(user_models::owner.eq(owner)),
        )
        .execute(db)
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;

    user_providers::delete()
        .where_(
            user_providers::id
                .eq(id)
                .and(user_providers::owner.eq(owner)),
        )
        .execute(db)
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    Ok(())
}

pub async fn list_user_models(
    db: &ConnectionPool,
    owner: Uuid,
) -> anyhow::Result<Vec<UserModelSummary>> {
    let rows = user_models::select()
        .where_(user_models::owner.eq(owner))
        .order_by_asc(user_models::name)
        .all(db)
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    let provider_names = provider_name_map(db, owner).await?;

    Ok(rows
        .into_iter()
        .map(|row| UserModelSummary {
            id: row.id.to_string(),
            name: row.name.clone(),
            slug: row.slug,
            display_name: model_display_name(&row.name, row.display_name.as_deref()),
            description: optional_text(row.description.as_deref()),
            provider_id: row.provider_id.to_string(),
            provider_name: provider_names
                .get(&row.provider_id)
                .cloned()
                .unwrap_or_else(|| "unknown".to_string()),
            vision: row.vision,
            reasoning_effort: row.reasoning_effort,
            created_at: row.created_at,
        })
        .collect())
}

pub async fn create_user_model(
    db: &ConnectionPool,
    owner: Uuid,
    input: UserModelInput,
) -> anyhow::Result<UserModelSummary> {
    let name = normalize_name(&input.name)?;
    let slug = input.slug.trim();
    if slug.is_empty() {
        anyhow::bail!("slug is required");
    }
    ensure_unique_model_name(db, owner, &name).await?;
    ensure_owned_provider(db, owner, input.provider_id).await?;

    let reasoning_effort = input
        .reasoning_effort
        .as_deref()
        .map(normalize_reasoning_effort)
        .transpose()?;
    let display_name = normalize_optional_string(input.display_name.as_deref());
    let description = normalize_optional_string(input.description.as_deref());

    let id = Uuid::now_v7();
    let created_at = now_secs();
    user_models::insert()
        .id(id)
        .owner(owner)
        .name(name.as_str())
        .slug(slug)
        .provider_id(input.provider_id)
        .display_name(display_name.as_deref())
        .description(description.as_deref())
        .reasoning_effort(reasoning_effort.as_deref())
        .vision(input.vision.unwrap_or(false))
        .created_at(created_at)
        .execute(db)
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;

    let provider_name = provider_name_map(db, owner)
        .await?
        .get(&input.provider_id)
        .cloned()
        .unwrap_or_else(|| "unknown".to_string());

    Ok(UserModelSummary {
        id: id.to_string(),
        name: name.clone(),
        slug: slug.to_string(),
        display_name: model_display_name(&name, display_name.as_deref()),
        description: optional_text(description.as_deref()),
        provider_id: input.provider_id.to_string(),
        provider_name,
        vision: input.vision.unwrap_or(false),
        reasoning_effort,
        created_at,
    })
}

pub async fn delete_user_model(db: &ConnectionPool, owner: Uuid, id: Uuid) -> anyhow::Result<()> {
    let existing = user_models::select_cols((user_models::id,))
        .where_(user_models::owner.eq(owner).and(user_models::id.eq(id)))
        .all(db)
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    if existing.is_empty() {
        anyhow::bail!("model not found");
    }

    user_models::delete()
        .where_(user_models::owner.eq(owner).and(user_models::id.eq(id)))
        .execute(db)
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    Ok(())
}

pub fn resolve_chat_model(
    registry: &ModelRegistry,
    requested: Option<&str>,
) -> Result<String, String> {
    let key = requested
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_MODEL);
    if registry.get(key).is_none() {
        return Err(format!("unknown model '{key}'"));
    }
    Ok(key.to_string())
}

fn entry_from_provider(
    kind: Kind,
    url: &str,
    token: &str,
    slug: &str,
    reasoning_effort: &Option<String>,
    vision: bool,
) -> ModelRegEntry {
    let api = match kind {
        Kind::OpenAI => OpenAI::new(url).into(),
        Kind::OpenRouter => OpenAI::openrouter(url).into(),
        Kind::Anthropic => Anthropic::new(url).into(),
        Kind::Ollama => Ollama::new(url).into(),
    };
    ModelRegEntry {
        api,
        token: token.to_string(),
        model_name: slug.to_string(),
        reasoning_effort: reasoning_effort.as_deref().and_then(parse_reasoning_effort),
        vision,
    }
}

fn parse_kind(value: &str) -> Option<Kind> {
    match value.to_ascii_lowercase().as_str() {
        "openai" => Some(Kind::OpenAI),
        "openrouter" => Some(Kind::OpenRouter),
        "anthropic" => Some(Kind::Anthropic),
        "ollama" => Some(Kind::Ollama),
        _ => None,
    }
}

fn normalize_kind(value: &str) -> anyhow::Result<String> {
    parse_kind(value)
        .ok_or_else(|| anyhow::anyhow!("unsupported provider kind"))
        .map(|_| value.trim().to_ascii_lowercase())
}

fn normalize_name(value: &str) -> anyhow::Result<String> {
    let name = value.trim();
    if name.is_empty() {
        anyhow::bail!("name is required");
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        anyhow::bail!("name may only contain letters, numbers, underscores, and hyphens");
    }
    Ok(name.to_string())
}

fn normalize_optional_string(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn model_display_name(registry_key: &str, display_name: Option<&str>) -> String {
    normalize_optional_string(display_name).unwrap_or_else(|| registry_key.to_string())
}

fn optional_text(value: Option<&str>) -> String {
    normalize_optional_string(value).unwrap_or_default()
}

fn normalize_url(value: &str) -> anyhow::Result<String> {
    let url = value.trim();
    if url.is_empty() {
        anyhow::bail!("url is required");
    }
    if !url.starts_with("http://") && !url.starts_with("https://") {
        anyhow::bail!("url must start with http:// or https://");
    }
    Ok(url.to_string())
}

fn normalize_reasoning_effort(value: &str) -> anyhow::Result<String> {
    parse_reasoning_effort(value)
        .ok_or_else(|| anyhow::anyhow!("invalid reasoning effort"))
        .map(|_| value.trim().to_ascii_lowercase())
}

fn parse_reasoning_effort(value: &str) -> Option<ReasoningEffort> {
    match value.to_ascii_lowercase().as_str() {
        "low" => Some(ReasoningEffort::Low),
        "medium" => Some(ReasoningEffort::Medium),
        "high" => Some(ReasoningEffort::High),
        "xhigh" => Some(ReasoningEffort::Xhigh),
        _ => None,
    }
}

fn reasoning_effort_name(effort: ReasoningEffort) -> String {
    match effort {
        ReasoningEffort::Low => "low",
        ReasoningEffort::Medium => "medium",
        ReasoningEffort::High => "high",
        ReasoningEffort::Xhigh => "xhigh",
    }
    .to_string()
}

fn parse_json_array(value: &str) -> Option<Vec<String>> {
    serde_json::from_str(value).ok()
}

async fn provider_name_map(
    db: &ConnectionPool,
    owner: Uuid,
) -> anyhow::Result<HashMap<Uuid, String>> {
    let rows = user_providers::select_cols((user_providers::id, user_providers::name))
        .where_(user_providers::owner.eq(owner))
        .all(db)
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    Ok(rows.into_iter().collect())
}

async fn ensure_unique_provider_name(
    db: &ConnectionPool,
    owner: Uuid,
    name: &str,
) -> anyhow::Result<()> {
    let rows = user_providers::select_cols((user_providers::id,))
        .where_(
            user_providers::owner
                .eq(owner)
                .and(user_providers::name.eq(name)),
        )
        .all(db)
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    if !rows.is_empty() {
        anyhow::bail!("a provider with this name already exists");
    }
    Ok(())
}

async fn ensure_unique_model_name(
    db: &ConnectionPool,
    owner: Uuid,
    name: &str,
) -> anyhow::Result<()> {
    let rows = user_models::select_cols((user_models::id,))
        .where_(user_models::owner.eq(owner).and(user_models::name.eq(name)))
        .all(db)
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    if !rows.is_empty() {
        anyhow::bail!("a model with this name already exists");
    }
    Ok(())
}

async fn ensure_owned_provider(
    db: &ConnectionPool,
    owner: Uuid,
    provider_id: Uuid,
) -> anyhow::Result<()> {
    let rows = user_providers::select_cols((user_providers::id,))
        .where_(
            user_providers::owner
                .eq(owner)
                .and(user_providers::id.eq(provider_id)),
        )
        .all(db)
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    if rows.is_empty() {
        anyhow::bail!("provider not found");
    }
    Ok(())
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, Server, ServerAgent};

    #[test]
    fn default_subagent_guidelines_read_from_config() {
        let mut config = Config {
            providers: HashMap::new(),
            models: HashMap::new(),
            server: Some(Server {
                db_url: None,
                db_path: None,
                listen_addr: None,
                allow_registration: None,
                ldap: None,
                files: None,
                telegram: None,
                github: None,
                google: None,
                public_url: None,
                agent: Some(ServerAgent {
                    default_subagent_guidelines: Some("Use fast models for lookups.".to_string()),
                }),
            }),
            tools: None,
            mcp: HashMap::new(),
        };
        assert_eq!(
            default_subagent_guidelines(&config),
            "Use fast models for lookups."
        );
        config.server = None;
        assert_eq!(default_subagent_guidelines(&config), "");
    }

    #[test]
    fn filters_internal_models_from_chat_list() {
        assert!(!is_chat_model(EMBEDDING_MODEL));
        assert!(!is_chat_model(TRANSCRIPTION_MODEL));
        assert!(is_chat_model("gpt_4_1"));
    }

    #[test]
    fn model_display_name_falls_back_to_registry_key() {
        assert_eq!(model_display_name("gpt_4_1", Some("GPT-4.1")), "GPT-4.1");
        assert_eq!(model_display_name("gpt_4_1", None), "gpt_4_1");
        assert_eq!(model_display_name("gpt_4_1", Some("  ")), "gpt_4_1");
    }
}
