//! Memory palace: a per-user, spatially organized long-term memory for the
//! cloud agent, inspired by MemPalace.
//!
//! The palace is organized into *wings* (top-level subjects such as a project,
//! person or topic), each holding *rooms* (specific topics) and *halls*
//! (conceptual categories). A memory is split in two: the verbatim original is
//! kept in a *drawer*, indexed by a *closet* — a compressed summary card that
//! also carries the vector embedding. Retrieval mirrors walking the palace:
//! the query is embedded, the closets are searched by meaning, and the matching
//! drawer's full content is pulled. *Doors* link rooms so a memory can be
//! reached from many angles.

use std::sync::Arc;

use async_trait::async_trait;
use llm::{Function, Tool as LlmTool};
use minisql::{ConnectionPool, Value};
use serde_json::{Value as JsonValue, json};
use stride_agent::{AgentConfig, Tool, ToolDesc};
use uuid::Uuid;

use stride_agent::memory::Embedding;

type DynError = Box<dyn std::error::Error + Send + Sync>;

const DEFAULT_RECALL_LIMIT: i64 = 5;
const MAX_RECALL_LIMIT: i64 = 20;

fn now_secs() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

/// Embed text with the registry's designated embedding model. Returns `None`
/// when no embedding model is configured or the provider call fails, so callers
/// can fall back to keyword search.
async fn embed(config: &AgentConfig, text: &str) -> Option<Embedding> {
    let entry = config.model_registry.embedding()?;
    match entry
        .api
        .get_embeddings(&entry.token, text, &entry.model_name)
        .await
    {
        Ok(response) => Some(Embedding::from_floats(&response.data.embedding)),
        Err(error) => {
            tracing::warn!(%error, "memory: embedding request failed, falling back to keywords");
            None
        }
    }
}

async fn scalar_uuid(
    db: &ConnectionPool,
    sql: &str,
    params: Vec<Value>,
) -> Result<Option<Uuid>, DynError> {
    let rows = db.query_with_params(sql, params).await?;
    match rows.rows().first() {
        Some(row) => Ok(Some(row.decode::<Uuid>("id")?)),
        None => Ok(None),
    }
}

async fn ensure_wing(
    db: &ConnectionPool,
    owner: Uuid,
    name: &str,
    description: &str,
) -> Result<Uuid, DynError> {
    if let Some(id) = scalar_uuid(
        db,
        "SELECT id FROM memory_wings WHERE owner = ? AND name = ? LIMIT 1",
        vec![Value::Uuid(owner), Value::Text(name.to_string())],
    )
    .await?
    {
        return Ok(id);
    }
    let id = Uuid::now_v7();
    db.query_with_params(
        "INSERT INTO memory_wings (id, owner, name, description, created_at) VALUES (?, ?, ?, ?, ?)",
        vec![
            Value::Uuid(id),
            Value::Uuid(owner),
            Value::Text(name.to_string()),
            Value::Text(description.to_string()),
            Value::Integer(now_secs()),
        ],
    )
    .await?;
    Ok(id)
}

async fn ensure_room(
    db: &ConnectionPool,
    owner: Uuid,
    wing: Uuid,
    name: &str,
    description: &str,
) -> Result<Uuid, DynError> {
    if let Some(id) = scalar_uuid(
        db,
        "SELECT id FROM memory_rooms WHERE owner = ? AND wing = ? AND name = ? LIMIT 1",
        vec![
            Value::Uuid(owner),
            Value::Uuid(wing),
            Value::Text(name.to_string()),
        ],
    )
    .await?
    {
        return Ok(id);
    }
    let id = Uuid::now_v7();
    db.query_with_params(
        "INSERT INTO memory_rooms (id, owner, wing, name, description, created_at) VALUES (?, ?, ?, ?, ?, ?)",
        vec![
            Value::Uuid(id),
            Value::Uuid(owner),
            Value::Uuid(wing),
            Value::Text(name.to_string()),
            Value::Text(description.to_string()),
            Value::Integer(now_secs()),
        ],
    )
    .await?;
    Ok(id)
}

async fn ensure_hall(
    db: &ConnectionPool,
    owner: Uuid,
    wing: Uuid,
    name: &str,
) -> Result<Uuid, DynError> {
    if let Some(id) = scalar_uuid(
        db,
        "SELECT id FROM memory_halls WHERE owner = ? AND wing = ? AND name = ? LIMIT 1",
        vec![
            Value::Uuid(owner),
            Value::Uuid(wing),
            Value::Text(name.to_string()),
        ],
    )
    .await?
    {
        return Ok(id);
    }
    let id = Uuid::now_v7();
    db.query_with_params(
        "INSERT INTO memory_halls (id, owner, wing, name, description, created_at) VALUES (?, ?, ?, ?, ?, ?)",
        vec![
            Value::Uuid(id),
            Value::Uuid(owner),
            Value::Uuid(wing),
            Value::Text(name.to_string()),
            Value::Text(String::new()),
            Value::Integer(now_secs()),
        ],
    )
    .await?;
    Ok(id)
}

async fn find_room(
    db: &ConnectionPool,
    owner: Uuid,
    wing: &str,
    room: &str,
) -> Result<Option<Uuid>, DynError> {
    scalar_uuid(
        db,
        "SELECT r.id AS id FROM memory_rooms r JOIN memory_wings w ON w.id = r.wing \
         WHERE r.owner = ? AND w.name = ? AND r.name = ? LIMIT 1",
        vec![
            Value::Uuid(owner),
            Value::Text(wing.to_string()),
            Value::Text(room.to_string()),
        ],
    )
    .await
}

/// Renders the palace structure as a system-prompt section so the agent knows
/// what it has stored and which tools to use, without searching first.
pub async fn palace_map(db: &ConnectionPool, owner: Uuid, project_wing: Option<&str>) -> String {
    let mut section = String::from(
        "## Memory Palace\n\n\
         You have a persistent personal memory that survives across conversations. Store durable \
         facts, decisions, preferences and context here, and recall them when relevant.\n\n\
         - `remember` — save a memory under a wing (subject) and room (topic), with a short summary and the full content.\n\
         - `recall` — search your memory by meaning to retrieve what you stored before.\n\
         - `explore_palace` — walk the palace to see which wings and rooms exist.\n\
         - `connect_memories` — link two related rooms.\n\n",
    );

    if let Some(wing) = project_wing {
        section.push_str(&format!(
            "This thread belongs to a project. Its memory wing is `{wing}` — `remember` and \
             `recall` default to it, so you can omit the wing. Name a different wing to store or \
             search elsewhere.\n\n"
        ));
    }

    let rows = db
        .query_with_params(
            "SELECT w.name AS wing, r.name AS room \
             FROM memory_wings w LEFT JOIN memory_rooms r ON r.wing = w.id \
             WHERE w.owner = ? ORDER BY w.name ASC, r.name ASC",
            vec![Value::Uuid(owner)],
        )
        .await;

    let Ok(rows) = rows else {
        return section;
    };

    if rows.is_empty() {
        section.push_str("Current palace: empty — nothing stored yet.\n");
        return section;
    }

    section.push_str("Current palace:\n");
    let mut current_wing: Option<String> = None;
    for row in rows.rows() {
        let wing = row.get_text("wing").unwrap_or_default().to_string();
        if current_wing.as_deref() != Some(wing.as_str()) {
            section.push_str(&format!("- {wing}\n"));
            current_wing = Some(wing);
        }
        if let Some(room) = row.get_text("room").filter(|r| !r.is_empty()) {
            section.push_str(&format!("  - {room}\n"));
        }
    }
    section
}

pub struct RememberTool {
    pub db: ConnectionPool,
    pub user_id: Uuid,
    /// Wing to file memories under when the agent does not name one. Set to the
    /// project's wing inside a project thread.
    pub default_wing: Option<String>,
}

pub struct RecallTool {
    pub db: ConnectionPool,
    pub user_id: Uuid,
    /// Wing searched when the agent does not name one. Set to the project's wing
    /// inside a project thread.
    pub default_wing: Option<String>,
}

pub struct ExplorePalaceTool {
    pub db: ConnectionPool,
    pub user_id: Uuid,
}

pub struct ConnectMemoriesTool {
    pub db: ConnectionPool,
    pub user_id: Uuid,
}

#[derive(ToolDesc)]
struct RememberParams {
    /// Top-level subject this memory belongs to (a project, person or theme), e.g. "stride-project".
    /// Omit it inside a project to file the memory under the project's own wing.
    wing: Option<String>,
    /// Specific topic within the wing, e.g. "auth-design".
    room: String,
    /// One or two sentence compressed summary of the memory. Used as the searchable index card.
    summary: String,
    /// The full, verbatim content worth remembering.
    content: String,
    /// Optional short title for the memory. Defaults to the summary.
    title: Option<String>,
    /// Optional conceptual category within the wing (e.g. "decisions", "preferences").
    hall: Option<String>,
    /// Optional comma-separated keywords to improve later recall.
    keywords: Option<String>,
}

#[derive(ToolDesc)]
struct RecallParams {
    /// What to look for, in natural language. Recall is unstructured — a vague description works.
    query: String,
    /// Optional wing to restrict the search to.
    wing: Option<String>,
    /// Maximum number of memories to return (default 5).
    limit: Option<i64>,
}

#[derive(ToolDesc)]
struct ExploreParams {
    /// Optional wing to look inside. Omit to list all wings.
    wing: Option<String>,
    /// Optional room within the wing to open. Omit to list the wing's rooms.
    room: Option<String>,
}

#[derive(ToolDesc)]
struct ConnectParams {
    /// Wing of the first room.
    from_wing: String,
    /// First room to link.
    from_room: String,
    /// Wing of the second room.
    to_wing: String,
    /// Second room to link.
    to_room: String,
    /// How the rooms relate, e.g. "depends on", "supersedes", "related".
    relation: String,
}

impl RememberTool {
    async fn store(
        &self,
        config: &AgentConfig,
        params: RememberParams,
    ) -> Result<JsonValue, DynError> {
        let owner = self.user_id;
        let wing = params
            .wing
            .clone()
            .filter(|w| !w.is_empty())
            .or_else(|| self.default_wing.clone())
            .ok_or("a wing is required: name a subject to file this memory under")?;
        let wing_id = ensure_wing(&self.db, owner, &wing, "").await?;
        let room_id = ensure_room(&self.db, owner, wing_id, &params.room, "").await?;
        let hall_id = match params.hall.as_deref().filter(|h| !h.is_empty()) {
            Some(hall) => Some(ensure_hall(&self.db, owner, wing_id, hall).await?),
            None => None,
        };

        let title = params
            .title
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| truncate(&params.summary, 80));

        let drawer_id = Uuid::now_v7();
        self.db
            .query_with_params(
                "INSERT INTO memory_drawers (id, owner, room, title, content, source, created_at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
                vec![
                    Value::Uuid(drawer_id),
                    Value::Uuid(owner),
                    Value::Uuid(room_id),
                    Value::Text(title.clone()),
                    Value::Text(params.content.clone()),
                    Value::Null,
                    Value::Integer(now_secs()),
                ],
            )
            .await?;

        let keywords = params.keywords.unwrap_or_default();
        let index_text = format!("{}\n{}", params.summary, keywords);
        let embedding = embed(config, index_text.trim()).await;
        let embedded = embedding.is_some();

        let closet_id = Uuid::now_v7();
        self.db
            .query_with_params(
                "INSERT INTO memory_closets (id, owner, room, drawer, hall, summary, keywords, embedding, created_at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                vec![
                    Value::Uuid(closet_id),
                    Value::Uuid(owner),
                    Value::Uuid(room_id),
                    Value::Uuid(drawer_id),
                    hall_id.map(Value::Uuid).unwrap_or(Value::Null),
                    Value::Text(params.summary.clone()),
                    Value::Text(keywords),
                    embedding.map(|e| e.into()).unwrap_or(Value::Null),
                    Value::Integer(now_secs()),
                ],
            )
            .await?;

        Ok(json!({
            "stored": true,
            "wing": wing,
            "room": params.room,
            "title": title,
            "embedded": embedded,
        }))
    }
}

#[async_trait(?Send)]
impl Tool for RememberTool {
    fn name(&self) -> &str {
        "remember"
    }

    fn readable_name(&self) -> &str {
        "Remember"
    }

    fn definition(&self) -> LlmTool {
        LlmTool {
            r#type: llm::ToolType::Function,
            function: Function {
                name: self.name().to_owned(),
                description: "Save a durable memory to your persistent memory palace. Choose a wing \
                              (subject) and room (topic), give a short summary used for search, and the \
                              full content. Use this whenever you learn something worth recalling later."
                    .to_string(),
                parameters: Some(RememberParams::function_parameters()),
            },
        }
    }

    async fn execute(&self, config: Arc<AgentConfig>, args: JsonValue) -> JsonValue {
        let params = match RememberParams::decode(args) {
            Ok(p) => p,
            Err(e) => return json!({"error": e}),
        };
        match self.store(&config, params).await {
            Ok(value) => value,
            Err(e) => json!({"error": e.to_string()}),
        }
    }
}

impl RecallTool {
    async fn vector_search(
        &self,
        query: &Embedding,
        wing: Option<&str>,
        limit: i64,
    ) -> Result<Vec<JsonValue>, DynError> {
        let mut sql = String::from(
            "SELECT w.name AS wing, r.name AS room, d.title AS title, c.summary AS summary, \
             d.content AS content, vec_distance_cosine(c.embedding, ?) AS dist \
             FROM memory_closets c \
             JOIN memory_drawers d ON d.id = c.drawer \
             JOIN memory_rooms r ON r.id = c.room \
             JOIN memory_wings w ON w.id = r.wing \
             WHERE c.owner = ? AND c.embedding IS NOT NULL",
        );
        let mut params = vec![Value::Blob(query.0.clone()), Value::Uuid(self.user_id)];
        if let Some(wing) = wing {
            sql.push_str(" AND w.name = ?");
            params.push(Value::Text(wing.to_string()));
        }
        sql.push_str(" ORDER BY dist ASC LIMIT ?");
        params.push(Value::Integer(limit));

        let rows = self.db.query_with_params(&sql, params).await?;
        Ok(rows
            .rows()
            .iter()
            .map(|row| {
                let dist = row.get_real("dist").unwrap_or(1.0);
                json!({
                    "wing": row.get_text("wing").unwrap_or_default(),
                    "room": row.get_text("room").unwrap_or_default(),
                    "title": row.get_text("title").unwrap_or_default(),
                    "summary": row.get_text("summary").unwrap_or_default(),
                    "content": row.get_text("content").unwrap_or_default(),
                    "score": (1.0 - dist).clamp(0.0, 1.0),
                })
            })
            .collect())
    }

    async fn keyword_search(
        &self,
        query: &str,
        wing: Option<&str>,
        limit: i64,
    ) -> Result<Vec<JsonValue>, DynError> {
        let pattern = format!("%{}%", query.to_lowercase());
        let mut sql = String::from(
            "SELECT w.name AS wing, r.name AS room, d.title AS title, c.summary AS summary, \
             d.content AS content \
             FROM memory_closets c \
             JOIN memory_drawers d ON d.id = c.drawer \
             JOIN memory_rooms r ON r.id = c.room \
             JOIN memory_wings w ON w.id = r.wing \
             WHERE c.owner = ? AND (lower(c.summary) LIKE ? OR lower(c.keywords) LIKE ? \
             OR lower(d.title) LIKE ? OR lower(d.content) LIKE ?)",
        );
        let mut params = vec![
            Value::Uuid(self.user_id),
            Value::Text(pattern.clone()),
            Value::Text(pattern.clone()),
            Value::Text(pattern.clone()),
            Value::Text(pattern),
        ];
        if let Some(wing) = wing {
            sql.push_str(" AND w.name = ?");
            params.push(Value::Text(wing.to_string()));
        }
        sql.push_str(" ORDER BY c.created_at DESC LIMIT ?");
        params.push(Value::Integer(limit));

        let rows = self.db.query_with_params(&sql, params).await?;
        Ok(rows
            .rows()
            .iter()
            .map(|row| {
                json!({
                    "wing": row.get_text("wing").unwrap_or_default(),
                    "room": row.get_text("room").unwrap_or_default(),
                    "title": row.get_text("title").unwrap_or_default(),
                    "summary": row.get_text("summary").unwrap_or_default(),
                    "content": row.get_text("content").unwrap_or_default(),
                })
            })
            .collect())
    }

    async fn recall(
        &self,
        config: &AgentConfig,
        params: RecallParams,
    ) -> Result<JsonValue, DynError> {
        let limit = params
            .limit
            .unwrap_or(DEFAULT_RECALL_LIMIT)
            .clamp(1, MAX_RECALL_LIMIT);
        // Default to the project's wing when the agent does not name one, so
        // recall stays focused on the current project but can still be widened
        // by naming another wing.
        let wing = params
            .wing
            .as_deref()
            .filter(|w| !w.is_empty())
            .or(self.default_wing.as_deref());

        let mut memories = match embed(config, &params.query).await {
            Some(embedding) => self.vector_search(&embedding, wing, limit).await?,
            None => Vec::new(),
        };

        // Fall back to keyword search when there is no embedding model or the
        // vector search found nothing.
        if memories.is_empty() {
            memories = self.keyword_search(&params.query, wing, limit).await?;
        }

        Ok(json!({ "found": memories.len(), "memories": memories }))
    }
}

#[async_trait(?Send)]
impl Tool for RecallTool {
    fn name(&self) -> &str {
        "recall"
    }

    fn readable_name(&self) -> &str {
        "Recall"
    }

    fn definition(&self) -> LlmTool {
        LlmTool {
            r#type: llm::ToolType::Function,
            function: Function {
                name: self.name().to_owned(),
                description: "Search your persistent memory palace by meaning and return the most \
                              relevant stored memories. Use a natural-language description — recall is \
                              unstructured, so a vague reference works."
                    .to_string(),
                parameters: Some(RecallParams::function_parameters()),
            },
        }
    }

    async fn execute(&self, config: Arc<AgentConfig>, args: JsonValue) -> JsonValue {
        let params = match RecallParams::decode(args) {
            Ok(p) => p,
            Err(e) => return json!({"error": e}),
        };
        match self.recall(&config, params).await {
            Ok(value) => value,
            Err(e) => json!({"error": e.to_string()}),
        }
    }
}

impl ExplorePalaceTool {
    async fn list_wings(&self) -> Result<JsonValue, DynError> {
        let rows = self
            .db
            .query_with_params(
                "SELECT w.name AS wing, w.description AS description, \
                 (SELECT COUNT(*) FROM memory_rooms r WHERE r.wing = w.id) AS rooms \
                 FROM memory_wings w WHERE w.owner = ? ORDER BY w.name ASC",
                vec![Value::Uuid(self.user_id)],
            )
            .await?;
        let wings: Vec<JsonValue> = rows
            .rows()
            .iter()
            .map(|row| {
                json!({
                    "wing": row.get_text("wing").unwrap_or_default(),
                    "description": row.get_text("description").unwrap_or_default(),
                    "rooms": row.get_int("rooms").unwrap_or(0),
                })
            })
            .collect();
        Ok(json!({ "wings": wings }))
    }

    async fn list_rooms(&self, wing: &str) -> Result<JsonValue, DynError> {
        let rows = self
            .db
            .query_with_params(
                "SELECT r.name AS room, r.description AS description, \
                 (SELECT COUNT(*) FROM memory_drawers d WHERE d.room = r.id) AS memories \
                 FROM memory_rooms r JOIN memory_wings w ON w.id = r.wing \
                 WHERE r.owner = ? AND w.name = ? ORDER BY r.name ASC",
                vec![Value::Uuid(self.user_id), Value::Text(wing.to_string())],
            )
            .await?;
        let rooms: Vec<JsonValue> = rows
            .rows()
            .iter()
            .map(|row| {
                json!({
                    "room": row.get_text("room").unwrap_or_default(),
                    "description": row.get_text("description").unwrap_or_default(),
                    "memories": row.get_int("memories").unwrap_or(0),
                })
            })
            .collect();
        let halls = self
            .db
            .query_with_params(
                "SELECT h.name AS hall FROM memory_halls h JOIN memory_wings w ON w.id = h.wing \
                 WHERE h.owner = ? AND w.name = ? ORDER BY h.name ASC",
                vec![Value::Uuid(self.user_id), Value::Text(wing.to_string())],
            )
            .await?;
        let halls: Vec<String> = halls
            .rows()
            .iter()
            .filter_map(|row| row.get_text("hall").map(str::to_string))
            .collect();
        Ok(json!({ "wing": wing, "rooms": rooms, "halls": halls }))
    }

    async fn open_room(&self, wing: &str, room: &str) -> Result<JsonValue, DynError> {
        let Some(room_id) = find_room(&self.db, self.user_id, wing, room).await? else {
            return Ok(json!({"error": format!("no room '{room}' in wing '{wing}'")}));
        };
        let rows = self
            .db
            .query_with_params(
                "SELECT d.title AS title, d.content AS content, \
                 (SELECT c.summary FROM memory_closets c WHERE c.drawer = d.id LIMIT 1) AS summary \
                 FROM memory_drawers d WHERE d.room = ? ORDER BY d.created_at DESC",
                vec![Value::Uuid(room_id)],
            )
            .await?;
        let memories: Vec<JsonValue> = rows
            .rows()
            .iter()
            .map(|row| {
                json!({
                    "title": row.get_text("title").unwrap_or_default(),
                    "summary": row.get_text("summary").unwrap_or_default(),
                    "content": row.get_text("content").unwrap_or_default(),
                })
            })
            .collect();
        let doors = self
            .db
            .query_with_params(
                "SELECT r2.name AS room, w2.name AS wing, dr.relation AS relation \
                 FROM memory_doors dr \
                 JOIN memory_rooms r2 ON r2.id = dr.to_room \
                 JOIN memory_wings w2 ON w2.id = r2.wing \
                 WHERE dr.owner = ? AND dr.from_room = ?",
                vec![Value::Uuid(self.user_id), Value::Uuid(room_id)],
            )
            .await?;
        let connected: Vec<JsonValue> = doors
            .rows()
            .iter()
            .map(|row| {
                json!({
                    "wing": row.get_text("wing").unwrap_or_default(),
                    "room": row.get_text("room").unwrap_or_default(),
                    "relation": row.get_text("relation").unwrap_or_default(),
                })
            })
            .collect();
        Ok(json!({ "wing": wing, "room": room, "memories": memories, "connected": connected }))
    }
}

#[async_trait(?Send)]
impl Tool for ExplorePalaceTool {
    fn name(&self) -> &str {
        "explore_palace"
    }

    fn readable_name(&self) -> &str {
        "Explore Palace"
    }

    fn definition(&self) -> LlmTool {
        LlmTool {
            r#type: llm::ToolType::Function,
            function: Function {
                name: self.name().to_owned(),
                description: "Walk your memory palace to see its structure. With no arguments lists all \
                              wings; with a wing lists its rooms and halls; with a wing and room opens \
                              the room and shows its memories and linked rooms."
                    .to_string(),
                parameters: Some(ExploreParams::function_parameters()),
            },
        }
    }

    async fn execute(&self, _config: Arc<AgentConfig>, args: JsonValue) -> JsonValue {
        let params = match ExploreParams::decode(args) {
            Ok(p) => p,
            Err(e) => return json!({"error": e}),
        };
        let wing = params.wing.filter(|w| !w.is_empty());
        let room = params.room.filter(|r| !r.is_empty());
        let result = match (wing, room) {
            (None, _) => self.list_wings().await,
            (Some(wing), None) => self.list_rooms(&wing).await,
            (Some(wing), Some(room)) => self.open_room(&wing, &room).await,
        };
        match result {
            Ok(value) => value,
            Err(e) => json!({"error": e.to_string()}),
        }
    }
}

impl ConnectMemoriesTool {
    async fn connect(&self, params: ConnectParams) -> Result<JsonValue, DynError> {
        let Some(from_id) =
            find_room(&self.db, self.user_id, &params.from_wing, &params.from_room).await?
        else {
            return Ok(
                json!({"error": format!("no room '{}' in wing '{}'", params.from_room, params.from_wing)}),
            );
        };
        let Some(to_id) =
            find_room(&self.db, self.user_id, &params.to_wing, &params.to_room).await?
        else {
            return Ok(
                json!({"error": format!("no room '{}' in wing '{}'", params.to_room, params.to_wing)}),
            );
        };

        self.db
            .query_with_params(
                "INSERT INTO memory_doors (id, owner, from_room, to_room, relation, created_at) \
                 VALUES (?, ?, ?, ?, ?, ?)",
                vec![
                    Value::Uuid(Uuid::now_v7()),
                    Value::Uuid(self.user_id),
                    Value::Uuid(from_id),
                    Value::Uuid(to_id),
                    Value::Text(params.relation.clone()),
                    Value::Integer(now_secs()),
                ],
            )
            .await?;
        Ok(json!({ "connected": true, "relation": params.relation }))
    }
}

#[async_trait(?Send)]
impl Tool for ConnectMemoriesTool {
    fn name(&self) -> &str {
        "connect_memories"
    }

    fn readable_name(&self) -> &str {
        "Connect Memories"
    }

    fn definition(&self) -> LlmTool {
        LlmTool {
            r#type: llm::ToolType::Function,
            function: Function {
                name: self.name().to_owned(),
                description:
                    "Create a door linking two existing rooms in your memory palace so a memory \
                              can be reached from a related topic."
                        .to_string(),
                parameters: Some(ConnectParams::function_parameters()),
            },
        }
    }

    async fn execute(&self, _config: Arc<AgentConfig>, args: JsonValue) -> JsonValue {
        let params = match ConnectParams::decode(args) {
            Ok(p) => p,
            Err(e) => return json!({"error": e}),
        };
        match self.connect(params).await {
            Ok(value) => value,
            Err(e) => json!({"error": e.to_string()}),
        }
    }
}

fn truncate(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    trimmed.chars().take(max_chars).collect::<String>() + "…"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    async fn setup() -> (ConnectionPool, Uuid) {
        let db = ConnectionPool::new("sqlite::memory:").unwrap();
        db::migrate(&db).await.unwrap();
        let user_id = Uuid::now_v7();
        db.query_with_params(
            "INSERT INTO users (id, username, password_hash) VALUES (?, ?, ?)",
            vec![
                Value::Uuid(user_id),
                Value::Text("alice".to_string()),
                Value::Text("hash".to_string()),
            ],
        )
        .await
        .unwrap();
        (db, user_id)
    }

    fn config() -> Arc<AgentConfig> {
        Arc::new(AgentConfig {
            model_registry: stride_agent::ModelRegistry::new(),
            max_iterations: 0,
        })
    }

    async fn remember(
        db: &ConnectionPool,
        user: Uuid,
        wing: &str,
        room: &str,
        summary: &str,
        content: &str,
    ) {
        let tool = RememberTool {
            db: db.clone(),
            user_id: user,
            default_wing: None,
        };
        let result = tool
            .execute(
                config(),
                json!({"wing": wing, "room": room, "summary": summary, "content": content}),
            )
            .await;
        assert_eq!(result["stored"], true, "remember failed: {result}");
    }

    #[tokio::test]
    async fn remember_then_recall_by_keyword() {
        let (db, user) = setup().await;
        remember(
            &db,
            user,
            "stride",
            "auth-design",
            "We chose JWT sessions over server-side cookies",
            "After weighing options we settled on stateless JWT sessions for the API.",
        )
        .await;

        let recall = RecallTool {
            db: db.clone(),
            user_id: user,
            default_wing: None,
        };
        let result = recall.execute(config(), json!({"query": "jwt"})).await;
        assert_eq!(result["found"], 1);
        assert_eq!(result["memories"][0]["room"], "auth-design");
        assert!(
            result["memories"][0]["content"]
                .as_str()
                .unwrap()
                .contains("stateless JWT")
        );
    }

    #[tokio::test]
    async fn recall_is_scoped_per_user() {
        let (db, user) = setup().await;
        remember(
            &db,
            user,
            "stride",
            "secrets",
            "private note",
            "very secret content",
        )
        .await;

        let other = Uuid::now_v7();
        db.query_with_params(
            "INSERT INTO users (id, username, password_hash) VALUES (?, ?, ?)",
            vec![
                Value::Uuid(other),
                Value::Text("bob".to_string()),
                Value::Text("hash".to_string()),
            ],
        )
        .await
        .unwrap();

        let recall = RecallTool {
            db: db.clone(),
            user_id: other,
            default_wing: None,
        };
        let result = recall.execute(config(), json!({"query": "secret"})).await;
        assert_eq!(result["found"], 0);
    }

    #[tokio::test]
    async fn explore_lists_wings_rooms_and_memories() {
        let (db, user) = setup().await;
        remember(
            &db,
            user,
            "stride",
            "auth-design",
            "jwt choice",
            "content one",
        )
        .await;
        remember(
            &db,
            user,
            "stride",
            "ci-pipeline",
            "use github actions",
            "content two",
        )
        .await;

        let explore = ExplorePalaceTool {
            db: db.clone(),
            user_id: user,
        };

        let wings = explore.execute(config(), json!({})).await;
        assert_eq!(wings["wings"][0]["wing"], "stride");
        assert_eq!(wings["wings"][0]["rooms"], 2);

        let rooms = explore.execute(config(), json!({"wing": "stride"})).await;
        assert_eq!(rooms["rooms"].as_array().unwrap().len(), 2);

        let room = explore
            .execute(config(), json!({"wing": "stride", "room": "auth-design"}))
            .await;
        assert_eq!(room["memories"][0]["content"], "content one");
    }

    #[tokio::test]
    async fn connect_links_rooms_and_shows_in_explore() {
        let (db, user) = setup().await;
        remember(&db, user, "stride", "auth-design", "jwt", "a").await;
        remember(&db, user, "stride", "api-design", "rest", "b").await;

        let connect = ConnectMemoriesTool {
            db: db.clone(),
            user_id: user,
        };
        let result = connect
            .execute(
                config(),
                json!({
                    "from_wing": "stride", "from_room": "auth-design",
                    "to_wing": "stride", "to_room": "api-design",
                    "relation": "depends on"
                }),
            )
            .await;
        assert_eq!(result["connected"], true);

        let explore = ExplorePalaceTool {
            db: db.clone(),
            user_id: user,
        };
        let room = explore
            .execute(config(), json!({"wing": "stride", "room": "auth-design"}))
            .await;
        assert_eq!(room["connected"][0]["room"], "api-design");
        assert_eq!(room["connected"][0]["relation"], "depends on");
    }

    #[tokio::test]
    async fn vector_search_orders_by_cosine_distance() {
        let (db, user) = setup().await;
        let wing = ensure_wing(&db, user, "stride", "").await.unwrap();
        let room = ensure_room(&db, user, wing, "vectors", "").await.unwrap();

        // Two memories pointing in different directions; the query leans toward
        // the first, so sqlite-vec must rank it ahead of the second.
        let cases = [
            ("x axis", "points along x", [1.0_f32, 0.0, 0.0]),
            ("y axis", "points along y", [0.0_f32, 1.0, 0.0]),
        ];
        for (summary, content, vector) in cases {
            let drawer = Uuid::now_v7();
            db.query_with_params(
                "INSERT INTO memory_drawers (id, owner, room, title, content, source, created_at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
                vec![
                    Value::Uuid(drawer),
                    Value::Uuid(user),
                    Value::Uuid(room),
                    Value::Text(summary.to_string()),
                    Value::Text(content.to_string()),
                    Value::Null,
                    Value::Integer(now_secs()),
                ],
            )
            .await
            .unwrap();
            db.query_with_params(
                "INSERT INTO memory_closets (id, owner, room, drawer, hall, summary, keywords, embedding, created_at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                vec![
                    Value::Uuid(Uuid::now_v7()),
                    Value::Uuid(user),
                    Value::Uuid(room),
                    Value::Uuid(drawer),
                    Value::Null,
                    Value::Text(summary.to_string()),
                    Value::Text(String::new()),
                    Embedding::from_floats(&vector).into(),
                    Value::Integer(now_secs()),
                ],
            )
            .await
            .unwrap();
        }

        let recall = RecallTool {
            db: db.clone(),
            user_id: user,
            default_wing: None,
        };
        let query = Embedding::from_floats(&[0.9_f32, 0.1, 0.0]);
        let results = recall.vector_search(&query, None, 5).await.unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0]["summary"], "x axis");
        assert!(
            results[0]["score"].as_f64().unwrap() > results[1]["score"].as_f64().unwrap(),
            "closer memory should score higher: {results:?}"
        );
    }

    #[tokio::test]
    async fn palace_map_reflects_contents() {
        let (db, user) = setup().await;
        let empty = palace_map(&db, user, None).await;
        assert!(empty.contains("empty"));

        remember(&db, user, "stride", "auth-design", "jwt", "a").await;
        let map = palace_map(&db, user, None).await;
        assert!(map.contains("- stride"));
        assert!(map.contains("  - auth-design"));
    }
}
