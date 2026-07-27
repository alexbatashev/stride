use std::{
    collections::{BTreeMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
};

use include_dir::{Dir, include_dir};
use minisql::ConnectionPool;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    db::skills as legacy_skills,
    vfs::{DirEntry, EntryKind, Vfs},
};

pub const SYSTEM_SKILLS_HOME: &str = "/usr/share/skills";
pub const USER_SKILLS_HOME: &str = "/home/user/skills";
pub const USER_SKILLS_ROOT: &str = "skills";
const MIGRATION_CONFLICTS_ROOT: &str = "skill-migration-conflicts";

static SYSTEM_SKILLS: Dir = include_dir!("$CARGO_MANIFEST_DIR/skills");

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SkillManifest {
    pub name: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compatibility: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<BTreeMap<String, String>>,
    #[serde(rename = "allowed-tools", skip_serializing_if = "Option::is_none")]
    pub allowed_tools: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkillOrigin {
    System,
    User,
}

#[derive(Clone, Debug)]
pub struct Skill {
    pub manifest: SkillManifest,
    pub content: String,
    pub path: String,
    pub origin: SkillOrigin,
}

#[derive(Clone, Debug, Serialize)]
pub struct UserSkillView {
    pub name: String,
    pub description: String,
    pub content: String,
    pub path: String,
    pub valid: bool,
    pub error: Option<String>,
}

#[derive(Debug)]
pub enum SkillStoreError {
    StorageUnavailable,
    Invalid(String),
    Conflict(String),
    NotFound(String),
    Internal(anyhow::Error),
}

impl std::fmt::Display for SkillStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StorageUnavailable => write!(f, "user skill storage is unavailable"),
            Self::Invalid(message) | Self::Conflict(message) | Self::NotFound(message) => {
                f.write_str(message)
            }
            Self::Internal(error) => write!(f, "{error:#}"),
        }
    }
}

impl std::error::Error for SkillStoreError {}

impl From<anyhow::Error> for SkillStoreError {
    fn from(error: anyhow::Error) -> Self {
        Self::Internal(error)
    }
}

#[derive(Clone)]
pub struct SkillStore {
    vfs: Option<Arc<Vfs>>,
}

impl SkillStore {
    pub fn new(vfs: Option<Arc<Vfs>>) -> Result<Self, SkillStoreError> {
        system_skills()?;
        Ok(Self { vfs })
    }

    pub async fn migrate_legacy(&self, db: &ConnectionPool) -> Result<(), SkillStoreError> {
        let rows = legacy_skills::select()
            .all(db)
            .await
            .map_err(|error| SkillStoreError::Internal(anyhow::anyhow!(error.to_string())))?;
        if let Some(row) = rows.iter().find(|row| row.owner.is_none()) {
            return Err(SkillStoreError::Invalid(format!(
                "legacy ownerless skill '{}' must be converted into server/skills/{}/SKILL.md",
                row.name, row.name
            )));
        }
        let Some(vfs) = self.vfs.as_ref() else {
            return Ok(());
        };

        for row in rows {
            let owner = row.owner.expect("ownerless rows rejected above");
            self.ensure_user_root(owner).await?;
            let manifest = SkillManifest {
                name: row.name.clone(),
                description: row.description.clone(),
                license: None,
                compatibility: None,
                metadata: None,
                allowed_tools: None,
            };
            let raw = match render_skill_file(&manifest, &row.content) {
                Ok(raw) => raw,
                Err(_) => {
                    let raw = render_legacy_backup(&manifest, &row.content)?;
                    self.back_up_legacy_row(vfs, owner, &row.id, &row.name, &raw)
                        .await?;
                    legacy_skills::delete()
                        .where_(legacy_skills::id.eq(row.id))
                        .execute(db)
                        .await
                        .map_err(|error| {
                            SkillStoreError::Internal(anyhow::anyhow!(error.to_string()))
                        })?;
                    continue;
                }
            };
            if find_system_skill(&row.name)?.is_some() {
                self.back_up_legacy_row(vfs, owner, &row.id, &row.name, &raw)
                    .await?;
                legacy_skills::delete()
                    .where_(legacy_skills::id.eq(row.id))
                    .execute(db)
                    .await
                    .map_err(|error| {
                        SkillStoreError::Internal(anyhow::anyhow!(error.to_string()))
                    })?;
                continue;
            }
            let path = user_manifest_path(&row.name);
            match vfs.read_global(owner, &path).await {
                Ok(existing) if existing != raw => {
                    self.back_up_legacy_row(vfs, owner, &row.id, &row.name, &raw)
                        .await?;
                }
                Ok(_) => {}
                Err(_) => {
                    vfs.create_dir_global(owner, &user_skill_rel(&row.name))
                        .await
                        .map_err(SkillStoreError::Internal)?;
                    vfs.write_global(owner, &path, &raw)
                        .await
                        .map_err(SkillStoreError::Internal)?;
                }
            }
            legacy_skills::delete()
                .where_(legacy_skills::id.eq(row.id))
                .execute(db)
                .await
                .map_err(|error| SkillStoreError::Internal(anyhow::anyhow!(error.to_string())))?;
        }
        Ok(())
    }

    async fn back_up_legacy_row(
        &self,
        vfs: &Vfs,
        owner: Uuid,
        id: &Uuid,
        name: &str,
        content: &str,
    ) -> Result<(), SkillStoreError> {
        let suffix = if validate_name(name).is_ok() {
            format!("-{name}")
        } else {
            String::new()
        };
        let dir = format!("{MIGRATION_CONFLICTS_ROOT}/{}{suffix}", id.as_simple());
        vfs.create_dir_global(owner, &dir)
            .await
            .map_err(SkillStoreError::Internal)?;
        vfs.write_global(owner, &format!("{dir}/SKILL.md"), content)
            .await
            .map_err(SkillStoreError::Internal)?;
        Ok(())
    }

    pub async fn ensure_user_root(&self, owner: Uuid) -> Result<(), SkillStoreError> {
        let vfs = self.vfs()?;
        vfs.create_dir_global(owner, USER_SKILLS_ROOT)
            .await
            .map_err(SkillStoreError::Internal)
    }

    pub async fn available(
        &self,
        owner: Uuid,
        excluded_system: &[String],
    ) -> Result<Vec<Skill>, SkillStoreError> {
        let excluded: HashSet<&str> = excluded_system.iter().map(String::as_str).collect();
        let mut skills: Vec<Skill> = system_skills()?
            .iter()
            .filter(|skill| !excluded.contains(skill.manifest.name.as_str()))
            .cloned()
            .collect();
        if self.vfs.is_some() {
            for view in self.user_views(owner).await? {
                if view.valid && find_system_skill(&view.name)?.is_none() {
                    skills.push(self.load_user(owner, &view.name).await?);
                }
            }
        }
        skills.sort_by(|left, right| left.manifest.name.cmp(&right.manifest.name));
        Ok(skills)
    }

    pub async fn user_views(&self, owner: Uuid) -> Result<Vec<UserSkillView>, SkillStoreError> {
        self.ensure_user_root(owner).await?;
        let vfs = self.vfs()?;
        let mut views = Vec::new();
        for entry in vfs
            .list_global(owner, USER_SKILLS_ROOT)
            .await
            .map_err(SkillStoreError::Internal)?
        {
            if !matches!(entry.kind, EntryKind::Directory) {
                continue;
            }
            let name = entry.name;
            let path = format!("{USER_SKILLS_HOME}/{name}");
            let result = self.load_user(owner, &name).await;
            match result {
                Ok(skill) if find_system_skill(&name)?.is_some() => views.push(UserSkillView {
                    name,
                    description: skill.manifest.description,
                    content: skill.content,
                    path,
                    valid: false,
                    error: Some("a built-in skill already uses this name".to_string()),
                }),
                Ok(skill) => views.push(UserSkillView {
                    name: skill.manifest.name,
                    description: skill.manifest.description,
                    content: skill.content,
                    path,
                    valid: true,
                    error: None,
                }),
                Err(SkillStoreError::NotFound(_)) => views.push(UserSkillView {
                    name,
                    description: String::new(),
                    content: String::new(),
                    path,
                    valid: false,
                    error: Some("missing SKILL.md".to_string()),
                }),
                Err(error @ SkillStoreError::Invalid(_)) => views.push(UserSkillView {
                    name,
                    description: String::new(),
                    content: String::new(),
                    path,
                    valid: false,
                    error: Some(error.to_string()),
                }),
                Err(error) => return Err(error),
            }
        }
        views.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(views)
    }

    async fn read_user_manifest(&self, owner: Uuid, name: &str) -> Result<String, SkillStoreError> {
        let vfs = self.vfs()?;
        let entries = vfs
            .list_global(owner, &user_skill_rel(name))
            .await
            .map_err(|error| {
                if error.to_string().contains("not found") {
                    SkillStoreError::NotFound(format!("skill '{name}' not found"))
                } else {
                    SkillStoreError::Internal(error)
                }
            })?;
        let Some(manifest) = entries.into_iter().find(|entry| entry.name == "SKILL.md") else {
            return Err(SkillStoreError::NotFound(format!(
                "skill '{name}' not found"
            )));
        };
        if !matches!(manifest.kind, EntryKind::File) {
            return Err(SkillStoreError::Invalid(
                "SKILL.md must be a file".to_string(),
            ));
        }
        let (bytes, _) = vfs
            .read_bytes_global(owner, &user_manifest_path(name))
            .await
            .map_err(SkillStoreError::Internal)?;
        String::from_utf8(bytes).map_err(|error| {
            SkillStoreError::Invalid(format!("SKILL.md is not valid UTF-8: {error}"))
        })
    }

    pub async fn load(
        &self,
        owner: Uuid,
        name: &str,
        excluded_system: &[String],
    ) -> Result<Skill, SkillStoreError> {
        validate_name(name)?;
        if let Some(skill) = find_system_skill(name)? {
            if excluded_system.iter().any(|excluded| excluded == name) {
                return Err(SkillStoreError::NotFound(format!(
                    "skill '{name}' not found"
                )));
            }
            return Ok(skill.clone());
        }
        self.load_user(owner, name).await
    }

    async fn load_user(&self, owner: Uuid, name: &str) -> Result<Skill, SkillStoreError> {
        self.ensure_user_root(owner).await?;
        let raw = self.read_user_manifest(owner, name).await?;
        parse_skill(
            &raw,
            name,
            SkillOrigin::User,
            &format!("{USER_SKILLS_HOME}/{name}"),
        )
    }

    pub async fn create(
        &self,
        owner: Uuid,
        name: &str,
        description: &str,
        content: &str,
    ) -> Result<Skill, SkillStoreError> {
        validate_name(name)?;
        if find_system_skill(name)?.is_some() {
            return Err(SkillStoreError::Conflict(format!(
                "'{name}' is a built-in skill and cannot be overwritten"
            )));
        }
        self.ensure_user_root(owner).await?;
        if self
            .vfs()?
            .list_global(owner, USER_SKILLS_ROOT)
            .await
            .map_err(SkillStoreError::Internal)?
            .iter()
            .any(|entry| entry.name == name)
        {
            return Err(SkillStoreError::Conflict(format!(
                "a skill directory named '{name}' already exists"
            )));
        }
        let manifest = SkillManifest {
            name: name.to_string(),
            description: description.trim().to_string(),
            license: None,
            compatibility: None,
            metadata: None,
            allowed_tools: None,
        };
        let raw = render_skill_file(&manifest, content)?;
        let vfs = self.vfs()?;
        vfs.create_dir_global(owner, &user_skill_rel(name))
            .await
            .map_err(SkillStoreError::Internal)?;
        vfs.write_global(owner, &user_manifest_path(name), &raw)
            .await
            .map_err(SkillStoreError::Internal)?;
        self.load_user(owner, name).await
    }

    pub async fn update(
        &self,
        owner: Uuid,
        name: &str,
        description: &str,
        content: &str,
    ) -> Result<Skill, SkillStoreError> {
        if find_system_skill(name)?.is_some() {
            return Err(SkillStoreError::Conflict(format!(
                "'{name}' is reserved by a built-in skill"
            )));
        }
        let existing = self.load_user(owner, name).await?;
        let mut manifest = existing.manifest;
        manifest.description = description.trim().to_string();
        let raw = render_skill_file(&manifest, content)?;
        self.vfs()?
            .write_global(owner, &user_manifest_path(name), &raw)
            .await
            .map_err(SkillStoreError::Internal)?;
        self.load_user(owner, name).await
    }

    pub async fn delete(&self, owner: Uuid, name: &str) -> Result<(), SkillStoreError> {
        validate_directory_name(name)?;
        self.ensure_user_root(owner).await?;
        let vfs = self.vfs()?;
        let exists = vfs
            .list_global(owner, USER_SKILLS_ROOT)
            .await
            .map_err(SkillStoreError::Internal)?
            .into_iter()
            .any(|entry| entry.name == name && matches!(entry.kind, EntryKind::Directory));
        if !exists {
            return Err(SkillStoreError::NotFound(format!(
                "skill '{name}' not found"
            )));
        }
        vfs.delete_global(owner, &user_skill_rel(name))
            .await
            .map_err(|_| SkillStoreError::NotFound(format!("skill '{name}' not found")))
    }

    fn vfs(&self) -> Result<&Vfs, SkillStoreError> {
        self.vfs
            .as_deref()
            .ok_or(SkillStoreError::StorageUnavailable)
    }
}

pub fn parse_skill(
    raw: &str,
    directory_name: &str,
    origin: SkillOrigin,
    path: &str,
) -> Result<Skill, SkillStoreError> {
    let raw = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    let rest = raw
        .strip_prefix("---\n")
        .or_else(|| raw.strip_prefix("---\r\n"))
        .ok_or_else(|| {
            SkillStoreError::Invalid("SKILL.md must start with YAML frontmatter".into())
        })?;
    let mut offset = 0;
    let mut parts = None;
    for line in rest.split_inclusive('\n') {
        let line_without_ending = line.strip_suffix('\n').unwrap_or(line);
        let line_without_ending = line_without_ending
            .strip_suffix('\r')
            .unwrap_or(line_without_ending);
        if line_without_ending == "---" {
            parts = Some((&rest[..offset], &rest[offset + line.len()..]));
            break;
        }
        offset += line.len();
    }
    if parts.is_none() && rest[offset..] == *"---" {
        parts = Some((&rest[..offset], ""));
    }
    let (frontmatter, body) = parts
        .ok_or_else(|| SkillStoreError::Invalid("SKILL.md frontmatter is not terminated".into()))?;
    let body = body
        .strip_prefix("\r\n")
        .or_else(|| body.strip_prefix('\n'))
        .unwrap_or(body);
    let manifest: SkillManifest = serde_yaml::from_str(frontmatter).map_err(|error| {
        SkillStoreError::Invalid(format!("invalid SKILL.md frontmatter: {error}"))
    })?;
    validate_manifest(&manifest, directory_name)?;
    Ok(Skill {
        manifest,
        content: body.to_string(),
        path: path.to_string(),
        origin,
    })
}

pub fn render_skill_file(
    manifest: &SkillManifest,
    content: &str,
) -> Result<String, SkillStoreError> {
    validate_manifest(manifest, &manifest.name)?;
    let yaml = serde_yaml::to_string(manifest)
        .map_err(|error| SkillStoreError::Invalid(format!("cannot render SKILL.md: {error}")))?;
    Ok(format!("---\n{yaml}---\n{content}"))
}

fn render_legacy_backup(
    manifest: &SkillManifest,
    content: &str,
) -> Result<String, SkillStoreError> {
    let yaml = serde_yaml::to_string(manifest).map_err(|error| {
        SkillStoreError::Invalid(format!("cannot back up legacy skill: {error}"))
    })?;
    Ok(format!("---\n{yaml}---\n{content}"))
}

fn validate_manifest(
    manifest: &SkillManifest,
    directory_name: &str,
) -> Result<(), SkillStoreError> {
    validate_name(&manifest.name)?;
    if manifest.name != directory_name {
        return Err(SkillStoreError::Invalid(format!(
            "skill name '{}' must match directory '{directory_name}'",
            manifest.name
        )));
    }
    let description_len = manifest.description.chars().count();
    if manifest.description.trim().is_empty() || description_len > 1024 {
        return Err(SkillStoreError::Invalid(
            "description must be 1-1024 characters".to_string(),
        ));
    }
    if let Some(compatibility) = &manifest.compatibility {
        let len = compatibility.chars().count();
        if compatibility.trim().is_empty() || len > 500 {
            return Err(SkillStoreError::Invalid(
                "compatibility must be 1-500 characters".to_string(),
            ));
        }
    }
    Ok(())
}

pub fn validate_name(name: &str) -> Result<(), SkillStoreError> {
    let len = name.chars().count();
    if len == 0 || len > 64 {
        return Err(SkillStoreError::Invalid(
            "name must be 1-64 characters".to_string(),
        ));
    }
    if name.starts_with('-') || name.ends_with('-') || name.contains("--") {
        return Err(SkillStoreError::Invalid(
            "name must not start or end with a hyphen or contain consecutive hyphens".to_string(),
        ));
    }
    if !name
        .bytes()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(SkillStoreError::Invalid(
            "name may only contain lowercase letters, numbers, and hyphens".to_string(),
        ));
    }
    Ok(())
}

fn validate_directory_name(name: &str) -> Result<(), SkillStoreError> {
    if name.is_empty()
        || matches!(name, "." | "..")
        || name.contains('/')
        || name.chars().any(char::is_control)
    {
        return Err(SkillStoreError::Invalid(
            "skill directory must be one direct child of /home/user/skills".to_string(),
        ));
    }
    Ok(())
}

pub fn system_list(path: &str) -> Result<Vec<DirEntry>, SkillStoreError> {
    let dir = if path.is_empty() {
        &SYSTEM_SKILLS
    } else {
        SYSTEM_SKILLS
            .get_dir(path)
            .ok_or_else(|| SkillStoreError::NotFound(path.to_string()))?
    };
    let mut entries = Vec::new();
    for child in dir.dirs() {
        entries.push(DirEntry {
            name: child
                .path()
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            kind: EntryKind::Directory,
            size: None,
            updated_at: 0,
            mime_type: None,
        });
    }
    for file in dir.files() {
        entries.push(DirEntry {
            name: file
                .path()
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            kind: EntryKind::File,
            size: Some(file.contents().len() as i64),
            updated_at: 0,
            mime_type: mime_for(file.path()).map(str::to_string),
        });
    }
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(entries)
}

pub fn system_read(path: &str) -> Result<(Vec<u8>, Option<String>), SkillStoreError> {
    let file = SYSTEM_SKILLS
        .get_file(path)
        .ok_or_else(|| SkillStoreError::NotFound(path.to_string()))?;
    Ok((
        file.contents().to_vec(),
        mime_for(file.path()).map(str::to_string),
    ))
}

pub async fn materialize_system_skills(root: &Path) -> anyhow::Result<()> {
    if tokio::fs::try_exists(root).await? {
        tokio::fs::remove_dir_all(root).await?;
    }
    tokio::fs::create_dir_all(root).await?;
    materialize_dir(&SYSTEM_SKILLS, root).await
}

fn materialize_dir<'a>(
    dir: &'a Dir<'a>,
    root: &'a Path,
) -> std::pin::Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + 'a>> {
    Box::pin(async move {
        for child in dir.dirs() {
            let child_root = root.join(child.path().file_name().unwrap_or_default());
            tokio::fs::create_dir_all(&child_root).await?;
            materialize_dir(child, &child_root).await?;
        }
        for file in dir.files() {
            let path = root.join(file.path().file_name().unwrap_or_default());
            tokio::fs::write(path, file.contents()).await?;
        }
        Ok(())
    })
}

pub async fn materialize_user_skills(vfs: &Vfs, owner: Uuid, root: &Path) -> anyhow::Result<()> {
    if tokio::fs::try_exists(root).await? {
        tokio::fs::remove_dir_all(root).await?;
    }
    tokio::fs::create_dir_all(root).await?;
    let mut stack = vec![(USER_SKILLS_ROOT.to_string(), PathBuf::new())];
    while let Some((vfs_dir, local_dir)) = stack.pop() {
        tokio::fs::create_dir_all(root.join(&local_dir)).await?;
        for entry in vfs.list_global(owner, &vfs_dir).await.unwrap_or_default() {
            let child_vfs = format!("{vfs_dir}/{}", entry.name);
            let child_local = local_dir.join(&entry.name);
            match entry.kind {
                EntryKind::Directory => stack.push((child_vfs, child_local)),
                EntryKind::File => {
                    let (bytes, _) = vfs.read_bytes_global(owner, &child_vfs).await?;
                    tokio::fs::write(root.join(child_local), bytes).await?;
                }
            }
        }
    }
    Ok(())
}

fn system_skills() -> Result<&'static [Skill], SkillStoreError> {
    static SKILLS: OnceLock<Result<Vec<Skill>, String>> = OnceLock::new();
    let result = SKILLS.get_or_init(|| {
        let mut skills = Vec::new();
        for dir in SYSTEM_SKILLS.dirs() {
            let name = dir
                .path()
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let file = dir
                .files()
                .find(|file| {
                    file.path()
                        .file_name()
                        .is_some_and(|name| name == "SKILL.md")
                })
                .ok_or_else(|| format!("server/skills/{name} is missing SKILL.md"))?;
            let raw = file
                .contents_utf8()
                .ok_or_else(|| format!("server/skills/{name}/SKILL.md is not UTF-8"))?;
            let skill = parse_skill(
                raw,
                &name,
                SkillOrigin::System,
                &format!("{SYSTEM_SKILLS_HOME}/{name}"),
            )
            .map_err(|error| format!("server/skills/{name}/SKILL.md: {error}"))?;
            skills.push(skill);
        }
        skills.sort_by(|left, right| left.manifest.name.cmp(&right.manifest.name));
        Ok(skills)
    });
    result
        .as_deref()
        .map_err(|message| SkillStoreError::Invalid(message.clone()))
}

fn find_system_skill(name: &str) -> Result<Option<&'static Skill>, SkillStoreError> {
    Ok(system_skills()?
        .iter()
        .find(|skill| skill.manifest.name == name))
}

fn user_skill_rel(name: &str) -> String {
    format!("{USER_SKILLS_ROOT}/{name}")
}

fn user_manifest_path(name: &str) -> String {
    format!("{}/{name}/SKILL.md", USER_SKILLS_ROOT)
}

fn mime_for(path: &Path) -> Option<&'static str> {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("md") => Some("text/markdown"),
        Some("py") => Some("text/x-python"),
        Some("sh") => Some("text/x-shellscript"),
        Some("json") => Some("application/json"),
        Some("yaml" | "yml") => Some("application/yaml"),
        Some("txt") => Some("text/plain"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use minisql::{ConnectionPool, Value};
    use tempfile::TempDir;

    async fn setup_store() -> (ConnectionPool, Arc<Vfs>, Arc<SkillStore>, Uuid, TempDir) {
        let db = ConnectionPool::new("sqlite::memory:").unwrap();
        db.initialize_database(crate::db::get_migrations())
            .await
            .unwrap();
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
        let dir = TempDir::new().unwrap();
        let provider = crate::vfs::AnyFileProvider::Local(
            crate::vfs::LocalFileProvider::with_id_gen(
                dir.path().to_path_buf(),
                Arc::new(stride_agent::SystemIdGen),
            )
            .unwrap(),
        );
        let vfs = Arc::new(Vfs::with_clock(
            db.clone(),
            provider,
            3,
            Arc::new(stride_agent::SystemClock),
            Arc::new(stride_agent::SystemIdGen),
        ));
        let store = Arc::new(SkillStore::new(Some(vfs.clone())).unwrap());
        (db, vfs, store, owner, dir)
    }

    #[test]
    fn validates_agent_skill_names() {
        for name in ["a", "pdf-processing", "data2"] {
            validate_name(name).unwrap();
        }
        for name in ["", "-bad", "bad-", "bad--name", "Bad", "has_space"] {
            assert!(validate_name(name).is_err(), "{name}");
        }
    }

    #[test]
    fn parses_and_renders_standard_manifest() {
        let raw = "---\nname: demo\ndescription: Use for demos.\nlicense: MIT\ncompatibility: Requires a POSIX shell.\nmetadata:\n  author: stride\nallowed-tools: Bash(cat:*) Read\n---\n\nDo it.";
        let skill = parse_skill(raw, "demo", SkillOrigin::User, "/home/user/skills/demo").unwrap();
        assert_eq!(skill.manifest.name, "demo");
        assert_eq!(skill.content, "Do it.");
        let rendered = render_skill_file(&skill.manifest, &skill.content).unwrap();
        let reparsed = parse_skill(
            &rendered,
            "demo",
            SkillOrigin::User,
            "/home/user/skills/demo",
        )
        .unwrap();
        assert_eq!(reparsed.manifest.license.as_deref(), Some("MIT"));
        assert_eq!(
            reparsed.manifest.compatibility.as_deref(),
            Some("Requires a POSIX shell.")
        );
        assert_eq!(
            reparsed.manifest.allowed_tools.as_deref(),
            Some("Bash(cat:*) Read")
        );
        assert_eq!(
            reparsed
                .manifest
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("author"))
                .map(String::as_str),
            Some("stride")
        );
    }

    #[test]
    fn rejects_empty_standard_text_fields_and_unknown_frontmatter() {
        for raw in [
            "---\nname: demo\ndescription: '   '\n---\nBody",
            "---\nname: demo\ndescription: Demo\ncompatibility: '   '\n---\nBody",
            "---\nname: demo\ndescription: Demo\ntitle: Legacy\n---\nBody",
        ] {
            assert!(parse_skill(raw, "demo", SkillOrigin::User, "/home/user/skills/demo").is_err());
        }
    }

    #[test]
    fn every_embedded_skill_is_valid() {
        let skills = system_skills().unwrap();
        assert!(!skills.is_empty());
        assert!(
            skills
                .iter()
                .any(|skill| skill.manifest.name == "skill-creation")
        );
        let skill_creation = system_read("skill-creation/SKILL.md").unwrap().0;
        assert!(
            String::from_utf8(skill_creation)
                .unwrap()
                .contains("/home/user/skills/<name>")
        );
        let manifest = system_read("pdf-report/SKILL.md").unwrap().0;
        assert!(
            String::from_utf8(manifest)
                .unwrap()
                .contains("%SKILL_HOME%")
        );
        let template = system_read("pdf-report/assets/report-template.typ")
            .unwrap()
            .0;
        assert!(
            String::from_utf8(template)
                .unwrap()
                .contains("= Report title")
        );
    }

    #[tokio::test]
    async fn user_bundles_are_discovered_and_optional_fields_survive_updates() {
        let (_db, vfs, store, owner, _dir) = setup_store().await;
        store
            .create(owner, "demo", "Use for demos.", "Original")
            .await
            .unwrap();
        vfs.write_global(owner, "skills/demo/scripts/run.py", "print('ok')")
            .await
            .unwrap();
        vfs.write_global(
            owner,
            "skills/manual/SKILL.md",
            "---\nname: manual\ndescription: Use manually.\nlicense: MIT\nmetadata:\n  author: stride\n---\n\nBefore",
        )
        .await
        .unwrap();

        let available = store.available(owner, &[]).await.unwrap();
        assert!(available.iter().any(|skill| skill.manifest.name == "demo"));
        assert!(
            available
                .iter()
                .any(|skill| skill.manifest.name == "manual")
        );
        store
            .update(owner, "manual", "Updated description.", "After")
            .await
            .unwrap();
        let updated = store.load(owner, "manual", &[]).await.unwrap();
        assert_eq!(updated.manifest.license.as_deref(), Some("MIT"));
        assert_eq!(updated.content, "After");
        assert_eq!(
            vfs.read_global(owner, "skills/demo/scripts/run.py")
                .await
                .unwrap(),
            "print('ok')"
        );
        let scheduled_snapshot = tempfile::tempdir().unwrap();
        materialize_user_skills(&vfs, owner, scheduled_snapshot.path())
            .await
            .unwrap();
        assert_eq!(
            tokio::fs::read_to_string(
                scheduled_snapshot
                    .path()
                    .join("demo")
                    .join("scripts")
                    .join("run.py")
            )
            .await
            .unwrap(),
            "print('ok')"
        );
        store.delete(owner, "demo").await.unwrap();
        assert!(
            vfs.read_global(owner, "skills/demo/scripts/run.py")
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn invalid_manual_bundles_are_diagnostics_not_catalog_entries() {
        let (_db, vfs, store, owner, _dir) = setup_store().await;
        vfs.write_global(
            owner,
            "skills/broken/SKILL.md",
            "---\nname: another-name\ndescription: Broken.\n---\n\nNo",
        )
        .await
        .unwrap();
        vfs.write_global(
            owner,
            "skills/Bad Name/SKILL.md",
            "---\nname: bad-name\ndescription: Broken directory.\n---\nNo",
        )
        .await
        .unwrap();
        vfs.write_bytes_global(
            owner,
            "skills/binary/SKILL.md",
            &[0xff, 0xfe],
            Some("text/markdown"),
        )
        .await
        .unwrap();

        let views = store.user_views(owner).await.unwrap();
        let broken = views.iter().find(|view| view.name == "broken").unwrap();
        assert!(!broken.valid);
        assert!(
            broken
                .error
                .as_deref()
                .unwrap()
                .contains("must match directory")
        );
        assert!(
            store
                .available(owner, &[])
                .await
                .unwrap()
                .iter()
                .all(|skill| skill.manifest.name != "broken")
        );
        let binary = views.iter().find(|view| view.name == "binary").unwrap();
        assert!(binary.error.as_deref().unwrap().contains("not valid UTF-8"));
        store.delete(owner, "Bad Name").await.unwrap();
        assert!(
            store
                .user_views(owner)
                .await
                .unwrap()
                .iter()
                .all(|view| view.name != "Bad Name")
        );
    }

    #[tokio::test]
    async fn built_in_names_are_reserved_and_cannot_be_shadowed() {
        let (_db, vfs, store, owner, _dir) = setup_store().await;
        vfs.write_global(
            owner,
            "skills/pdf-report/SKILL.md",
            "---\nname: pdf-report\ndescription: User collision.\n---\nUser body",
        )
        .await
        .unwrap();

        let view = store
            .user_views(owner)
            .await
            .unwrap()
            .into_iter()
            .find(|view| view.name == "pdf-report")
            .unwrap();
        assert!(!view.valid);
        assert!(view.error.unwrap().contains("built-in"));
        assert_eq!(
            store.load(owner, "pdf-report", &[]).await.unwrap().origin,
            SkillOrigin::System
        );
        assert!(matches!(
            store
                .load(owner, "pdf-report", &["pdf-report".into()])
                .await,
            Err(SkillStoreError::NotFound(_))
        ));
        assert!(matches!(
            store
                .update(owner, "pdf-report", "Changed", "Changed")
                .await,
            Err(SkillStoreError::Conflict(_))
        ));
    }

    #[tokio::test]
    async fn discovery_is_live_isolated_and_reports_directory_renames() {
        let (db, vfs, store, owner, _dir) = setup_store().await;
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
        store
            .create(owner, "live", "First description", "Body")
            .await
            .unwrap();
        assert!(
            store
                .available(other, &[])
                .await
                .unwrap()
                .iter()
                .all(|skill| skill.manifest.name != "live")
        );

        vfs.write_global(
            owner,
            "skills/live/SKILL.md",
            "---\nname: live\ndescription: Changed through Files.\n---\nNew body",
        )
        .await
        .unwrap();
        let refreshed = store.load(owner, "live", &[]).await.unwrap();
        assert_eq!(refreshed.manifest.description, "Changed through Files.");
        assert_eq!(refreshed.content, "New body");

        vfs.rename_global(owner, "skills/live", "renamed")
            .await
            .unwrap();
        let renamed = store
            .user_views(owner)
            .await
            .unwrap()
            .into_iter()
            .find(|view| view.name == "renamed")
            .unwrap();
        assert!(!renamed.valid);
        assert!(renamed.error.unwrap().contains("must match directory"));
    }

    #[tokio::test]
    async fn legacy_rows_migrate_once_without_overwriting_filesystem_bundles() {
        let (db, vfs, store, owner, _dir) = setup_store().await;
        let migrated_id = Uuid::now_v7();
        legacy_skills::insert()
            .id(migrated_id)
            .name("legacy")
            .title("Legacy title")
            .description("Legacy description")
            .content("Legacy body")
            .owner(Some(owner))
            .execute(&db)
            .await
            .unwrap();
        let conflict_id = Uuid::now_v7();
        legacy_skills::insert()
            .id(conflict_id)
            .name("existing")
            .title("Existing title")
            .description("Database description")
            .content("Database body")
            .owner(Some(owner))
            .execute(&db)
            .await
            .unwrap();
        let reserved_id = Uuid::now_v7();
        legacy_skills::insert()
            .id(reserved_id)
            .name("pdf-report")
            .title("Old PDF report")
            .description("Database built-in collision")
            .content("Database PDF body")
            .owner(Some(owner))
            .execute(&db)
            .await
            .unwrap();
        store
            .create(
                owner,
                "existing",
                "Filesystem description",
                "Filesystem body",
            )
            .await
            .unwrap();

        store.migrate_legacy(&db).await.unwrap();
        store.migrate_legacy(&db).await.unwrap();

        assert_eq!(
            store.load(owner, "legacy", &[]).await.unwrap().content,
            "Legacy body"
        );
        assert_eq!(
            store.load(owner, "existing", &[]).await.unwrap().content,
            "Filesystem body"
        );
        assert!(
            vfs.read_global(
                owner,
                &format!(
                    "{MIGRATION_CONFLICTS_ROOT}/{}-existing/SKILL.md",
                    conflict_id.as_simple()
                )
            )
            .await
            .unwrap()
            .contains("Database body")
        );
        assert!(
            vfs.read_global(
                owner,
                &format!(
                    "{MIGRATION_CONFLICTS_ROOT}/{}-pdf-report/SKILL.md",
                    reserved_id.as_simple()
                )
            )
            .await
            .unwrap()
            .contains("Database PDF body")
        );
        assert!(
            store
                .user_views(owner)
                .await
                .unwrap()
                .iter()
                .all(|view| view.name != "pdf-report")
        );
        assert!(legacy_skills::select().all(&db).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn migration_defers_without_vfs_and_rejects_ownerless_rows() {
        let (db, _vfs, _store, owner, _dir) = setup_store().await;
        let deferred_id = Uuid::now_v7();
        legacy_skills::insert()
            .id(deferred_id)
            .name("deferred")
            .title("Deferred")
            .description("Deferred description")
            .content("Deferred body")
            .owner(Some(owner))
            .execute(&db)
            .await
            .unwrap();
        SkillStore::new(None)
            .unwrap()
            .migrate_legacy(&db)
            .await
            .unwrap();
        assert_eq!(legacy_skills::select().all(&db).await.unwrap().len(), 1);

        let ownerless_id = Uuid::now_v7();
        legacy_skills::insert()
            .id(ownerless_id)
            .name("ownerless")
            .title("Ownerless")
            .description("Ownerless description")
            .content("Ownerless body")
            .owner(None)
            .execute(&db)
            .await
            .unwrap();
        let error = SkillStore::new(None)
            .unwrap()
            .migrate_legacy(&db)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("ownerless"));
        assert_eq!(legacy_skills::select().all(&db).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn invalid_legacy_names_are_backed_up_under_a_safe_path() {
        let (db, vfs, store, owner, _dir) = setup_store().await;
        let id = Uuid::now_v7();
        legacy_skills::insert()
            .id(id)
            .name("../bad")
            .title("Bad")
            .description("Legacy description")
            .content("Legacy body")
            .owner(Some(owner))
            .execute(&db)
            .await
            .unwrap();

        store.migrate_legacy(&db).await.unwrap();

        let backup = vfs
            .read_global(
                owner,
                &format!("{MIGRATION_CONFLICTS_ROOT}/{}/SKILL.md", id.as_simple()),
            )
            .await
            .unwrap();
        assert!(backup.contains("name: ../bad"));
        assert!(backup.contains("Legacy body"));
        assert!(legacy_skills::select().all(&db).await.unwrap().is_empty());
    }
}
