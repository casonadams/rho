use std::fmt;
use std::path::Path;

include!(concat!(env!("OUT_DIR"), "/builtin_skills.rs"));

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillMetadata {
    pub name: String,
    pub description: String,
    pub location: String,
}

/// Where a resolved skill came from; `Project` overrides `User` overrides
/// `Builtin` for the same skill name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillOrigin {
    Builtin,
    User,
    Project,
}

impl fmt::Display for SkillOrigin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Builtin => write!(f, "built-in"),
            Self::User => write!(f, "user"),
            Self::Project => write!(f, "project"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSkill {
    pub metadata: SkillMetadata,
    pub origin: SkillOrigin,
}

pub fn builtin_skills() -> Vec<SkillMetadata> {
    BUILTIN_SKILLS
        .iter()
        .map(|skill| SkillMetadata {
            name: skill.name.to_string(),
            description: skill.description.to_string(),
            location: format!("rho://skills/{}", skill.name),
        })
        .collect()
}

pub fn get_builtin_skill_content(name: &str) -> Option<&'static str> {
    let clean = name
        .trim()
        .trim_start_matches("rho://skills/")
        .trim_end_matches("/SKILL.md")
        .trim_end_matches(".md");
    BUILTIN_SKILLS
        .iter()
        .find(|skill| skill.name == clean)
        .map(|skill| skill.content)
}

/// Resolve every skill available to the session: embedded built-ins, then
/// declarative overrides as `SKILL.md` files under `<config_dir>/skills` and
/// project skill directories. Later origins replace earlier ones by name.
/// Overrides carry readable content only and are never executed.
pub fn resolved_skills(config_dir: Option<&Path>, project_dir: Option<&Path>) -> Vec<ResolvedSkill> {
    let mut resolved: Vec<ResolvedSkill> = builtin_skills()
        .into_iter()
        .map(|metadata| ResolvedSkill {
            metadata,
            origin: SkillOrigin::Builtin,
        })
        .collect();
    if let Some(config_dir) = config_dir {
        scan_directory(&config_dir.join("skills"), SkillOrigin::User, &mut resolved);
    }
    if let Some(project_dir) = project_dir {
        scan_directory(&project_dir.join(".rho/skills"), SkillOrigin::Project, &mut resolved);
        scan_directory(&project_dir.join("prompts/skills"), SkillOrigin::Project, &mut resolved);
        scan_directory(&project_dir.join("skills"), SkillOrigin::Project, &mut resolved);
    }
    resolved.sort_by(|left, right| left.metadata.name.cmp(&right.metadata.name));
    resolved
}

/// Full content of one skill by name in the resolved set.
///
/// Built-in content is embedded; overrides are read from their recorded file
/// location, never interpreted or executed.
pub fn resolved_content(skills: &[ResolvedSkill], name: &str) -> Option<String> {
    let skill = skills.iter().find(|skill| skill.metadata.name == name)?;
    if skill.metadata.location.starts_with("rho://skills/") {
        get_builtin_skill_content(name).map(str::to_string)
    } else {
        std::fs::read_to_string(&skill.metadata.location).ok()
    }
}

fn scan_directory(directory: &Path, origin: SkillOrigin, resolved: &mut Vec<ResolvedSkill>) {
    if !directory.exists() || !directory.is_dir() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().map(|entry| entry.path()).collect();
    entries.sort();
    for path in entries {
        let skill_file = if path.is_dir() {
            path.join("SKILL.md")
        } else if path.extension().is_some_and(|ext| ext == "md") {
            path.clone()
        } else {
            continue;
        };
        if !skill_file.is_file() {
            continue;
        }
        if let Some(metadata) = parse_skill_file(&skill_file) {
            upsert_by_name(resolved, origin, metadata);
        }
    }
}

fn upsert_by_name(resolved: &mut Vec<ResolvedSkill>, origin: SkillOrigin, metadata: SkillMetadata) {
    match resolved.iter_mut().find(|skill| skill.metadata.name == metadata.name) {
        // A same-name skill from a later root replaces the earlier copy.
        Some(existing) => {
            existing.metadata = metadata;
            existing.origin = origin;
        }
        None => resolved.push(ResolvedSkill { metadata, origin }),
    }
}

fn parse_skill_file(path: &Path) -> Option<SkillMetadata> {
    let content = std::fs::read_to_string(path).ok()?;
    let declared_name = if path.file_name().is_some_and(|name| name == "SKILL.md") {
        // Directory skills: `<name>/SKILL.md` is named for the directory.
        path.parent()
            .and_then(|parent| parent.file_name())
            .and_then(|name| name.to_str())
            .map(str::to_string)
    } else {
        // Flat files: `<name>.md` is named for its file stem.
        path.file_stem().and_then(|name| name.to_str()).map(str::to_string)
    };
    Some(build_metadata(path, declared_name, &content))
}

const FALLBACK_DESCRIPTION: &str = "Custom agent skill";

fn build_metadata(path: &Path, declared_name: Option<String>, content: &str) -> SkillMetadata {
    let mut name = declared_name.unwrap_or_else(|| "skill".to_string());
    let mut description = String::new();

    if content.starts_with("---") {
        let parts: Vec<&str> = content.splitn(3, "---").collect();
        if parts.len() >= 3 {
            for line in parts[1].lines() {
                let trimmed = line.trim();
                if let Some(value) = trimmed.strip_prefix("name:") {
                    name = value.trim().trim_matches('"').trim_matches('\'').to_string();
                } else if let Some(value) = trimmed.strip_prefix("description:") {
                    description = value.trim().trim_matches('"').trim_matches('\'').to_string();
                }
            }
        }
    }

    if description.is_empty() {
        description = content
            .lines()
            .find(|line| !line.trim().is_empty() && !line.starts_with('#') && !line.starts_with("---"))
            .unwrap_or(FALLBACK_DESCRIPTION)
            .trim()
            .to_string();
    }

    SkillMetadata {
        name,
        description,
        location: path.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct SkillFixture {
        root: PathBuf,
        config_dir: PathBuf,
        project_dir: PathBuf,
    }

    impl Drop for SkillFixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn fixture() -> SkillFixture {
        let root = std::env::temp_dir().join(format!("skills_{}", uuid::Uuid::new_v4()));
        let config_dir = root.join("config");
        let project_dir = root.join("project");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::create_dir_all(&project_dir).unwrap();
        SkillFixture {
            root,
            config_dir,
            project_dir,
        }
    }

    fn write_builtin_override(dir: &Path, name: &str, body: &str) {
        let skill_dir = dir.join(name);
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(skill_dir.join("SKILL.md"), body).unwrap();
    }

    #[test]
    fn builtins_resolve_with_builtin_origin_and_content() {
        let resolved = resolved_skills(None, None);
        let plan = resolved
            .iter()
            .find(|skill| skill.metadata.name == "plan")
            .expect("plan is an embedded built-in");
        assert_eq!(plan.origin, SkillOrigin::Builtin);
        assert_eq!(
            resolved_content(&resolved, "plan").unwrap(),
            get_builtin_skill_content("plan").unwrap()
        );
    }

    #[test]
    fn user_override_replaces_same_name_builtin() {
        let fixture = fixture();
        write_builtin_override(
            &fixture.config_dir.join("skills"),
            "plan",
            "---\nname: plan\ndescription: User plan override\n---\n# Custom Plan\n",
        );

        let resolved = resolved_skills(Some(&fixture.config_dir), None);
        let plan = resolved.iter().find(|skill| skill.metadata.name == "plan").unwrap();
        assert_eq!(plan.origin, SkillOrigin::User);
        assert_eq!(plan.metadata.description, "User plan override");
        assert!(plan.metadata.location.contains("config/skills/plan/SKILL.md"));
        assert_eq!(
            resolved_content(&resolved, "plan").unwrap(),
            "---\nname: plan\ndescription: User plan override\n---\n# Custom Plan\n"
        );
    }

    #[test]
    fn project_override_beats_user_and_user_additions_survive() {
        let fixture = fixture();
        write_builtin_override(
            &fixture.config_dir.join("skills"),
            "plan",
            "---\nname: plan\ndescription: User plan\n---\n# User Plan\n",
        );
        write_builtin_override(
            &fixture.project_dir.join(".rho/skills"),
            "plan",
            "---\nname: plan\ndescription: Project plan\n---\n# Project Plan\n",
        );
        write_builtin_override(
            &fixture.config_dir.join("skills"),
            "team-notes",
            "---\nname: team-notes\ndescription: User notes workflow\n---\n# Notes\n",
        );

        let resolved = resolved_skills(Some(&fixture.config_dir), Some(&fixture.project_dir));
        let plan = resolved.iter().find(|skill| skill.metadata.name == "plan").unwrap();
        assert_eq!(plan.origin, SkillOrigin::Project);
        assert_eq!(plan.metadata.description, "Project plan");
        assert!(resolved_content(&resolved, "plan").unwrap().contains("# Project Plan"));

        let notes = resolved
            .iter()
            .find(|skill| skill.metadata.name == "team-notes")
            .unwrap();
        assert_eq!(notes.origin, SkillOrigin::User);
        assert!(resolved_content(&resolved, "team-notes").unwrap().contains("# Notes"));
    }

    #[test]
    fn flat_skill_files_use_their_file_stem_as_name() {
        let fixture = fixture();
        let skills_dir = fixture.project_dir.join(".rho/skills");
        std::fs::create_dir_all(&skills_dir).unwrap();
        std::fs::write(skills_dir.join("deploy.md"), "# Deploy workflow\nPush builds.\n").unwrap();

        let resolved = resolved_skills(None, Some(&fixture.project_dir));
        let deploy = resolved
            .iter()
            .find(|skill| skill.metadata.name == "deploy")
            .expect("flat file stem becomes the skill name");
        assert_eq!(deploy.origin, SkillOrigin::Project);
        assert_eq!(deploy.metadata.description, "Push builds.");
        assert!(resolved_content(&resolved, "deploy").unwrap().contains("Push builds."));
    }
}
