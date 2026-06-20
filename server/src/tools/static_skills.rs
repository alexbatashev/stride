use std::collections::HashSet;
use std::sync::OnceLock;

use include_dir::{Dir, include_dir};
use serde::Deserialize;

/// Skills baked into the binary at build time. They are surfaced to the agent
/// through `search_skills` and `load_skill`, but they are never written to the
/// database, so users cannot view, edit, or delete them.
static SKILL_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/skills");

pub struct StaticSkill {
    pub name: String,
    pub title: String,
    pub description: String,
    pub content: String,
}

#[derive(Deserialize)]
struct Frontmatter {
    name: String,
    title: String,
    description: String,
}

/// All embedded skills, parsed once and cached.
pub fn static_skills() -> &'static [StaticSkill] {
    static SKILLS: OnceLock<Vec<StaticSkill>> = OnceLock::new();
    SKILLS.get_or_init(load_skills)
}

pub fn find_static_skill(name: &str) -> Option<&'static StaticSkill> {
    static_skills().iter().find(|skill| skill.name == name)
}

/// Names of all embedded skills. Used to keep database skills from shadowing them.
pub fn static_skill_names() -> &'static HashSet<String> {
    static NAMES: OnceLock<HashSet<String>> = OnceLock::new();
    NAMES.get_or_init(|| static_skills().iter().map(|s| s.name.clone()).collect())
}

pub fn skill_matches_query(skill: &StaticSkill, query: &str) -> bool {
    skill.name.to_lowercase().contains(query)
        || skill.title.to_lowercase().contains(query)
        || skill.description.to_lowercase().contains(query)
}

fn load_skills() -> Vec<StaticSkill> {
    let mut skills: Vec<StaticSkill> = SKILL_DIR
        .files()
        .filter(is_markdown)
        .filter_map(|file| file.contents_utf8())
        .filter_map(parse_skill)
        .collect();
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    skills
}

fn is_markdown(file: &&include_dir::File) -> bool {
    file.path().extension().and_then(|ext| ext.to_str()) == Some("md")
}

/// Splits a `+++` TOML frontmatter block from the Markdown body.
fn parse_skill(raw: &str) -> Option<StaticSkill> {
    let raw = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    let mut lines = raw.lines();
    if lines.next()?.trim() != "+++" {
        return None;
    }

    let mut frontmatter = String::new();
    let mut closed = false;
    for line in lines.by_ref() {
        if line.trim() == "+++" {
            closed = true;
            break;
        }
        frontmatter.push_str(line);
        frontmatter.push('\n');
    }
    if !closed {
        return None;
    }

    let body = lines.collect::<Vec<_>>().join("\n");
    let meta: Frontmatter = toml::from_str(&frontmatter).ok()?;
    Some(StaticSkill {
        name: meta.name,
        title: meta.title,
        description: meta.description,
        content: body.trim().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_toml_frontmatter() {
        let raw = "+++\nname = \"demo\"\ntitle = \"Demo\"\ndescription = \"A demo skill.\"\n+++\n# Body\n\nDo the thing.";
        let skill = parse_skill(raw).unwrap();
        assert_eq!(skill.name, "demo");
        assert_eq!(skill.title, "Demo");
        assert_eq!(skill.description, "A demo skill.");
        assert_eq!(skill.content, "# Body\n\nDo the thing.");
    }

    #[test]
    fn rejects_missing_frontmatter() {
        assert!(parse_skill("# Just a heading").is_none());
    }

    #[test]
    fn rejects_unterminated_frontmatter() {
        assert!(parse_skill("+++\nname = \"x\"\nstill going").is_none());
    }

    #[test]
    fn every_embedded_skill_parses() {
        let shipped = SKILL_DIR.files().filter(is_markdown).count();
        assert_eq!(
            static_skills().len(),
            shipped,
            "an embedded skill file failed to parse"
        );
    }
}
