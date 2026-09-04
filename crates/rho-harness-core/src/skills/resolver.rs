use super::parser::parse_skill_file;
use super::types::{ResolvedSkill, SkillMetadata, SkillOrigin, SkillResolutionPaths};
use std::path::{Path, PathBuf};

/// Resolve every skill available to the session: declarative skills as `SKILL.md`
/// files under user directories (`~/.agents/skills`, `~/.config/agents/skills`,
/// `<config_dir>/skills`) and project skill directories (`.agents/skills`,
/// `.rho/skills`, `skills`). Project skills replace user skills by name.
/// Skills carry readable content only and are never executed.
pub fn resolved_skills(config_dir: Option<&Path>, project_dir: Option<&Path>) -> Vec<ResolvedSkill> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
        .map(PathBuf::from);
    let paths = SkillResolutionPaths {
        config_dir,
        project_dir,
        home_dir: home.as_deref(),
    };
    resolved_skills_for_paths(paths)
}

/// Resolve skills with an explicit user home directory.
pub fn resolved_skills_for_paths(paths: SkillResolutionPaths<'_>) -> Vec<ResolvedSkill> {
    let mut resolved: Vec<ResolvedSkill> = Vec::new();

    if let Some(home_path) = paths.home_dir {
        scan_directory(&home_path.join(".agents/skills"), SkillOrigin::User, &mut resolved);
        let xdg_skills = std::env::var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| home_path.join(".config"))
            .join("agents/skills");
        scan_directory(&xdg_skills, SkillOrigin::User, &mut resolved);
        scan_directory(&home_path.join(".skills"), SkillOrigin::User, &mut resolved);
    }
    if let Some(config_dir) = paths.config_dir {
        scan_directory(&config_dir.join("skills"), SkillOrigin::User, &mut resolved);
    }
    if let Some(project_dir) = paths.project_dir {
        scan_directory(&project_dir.join(".agents/skills"), SkillOrigin::Project, &mut resolved);
        scan_directory(&project_dir.join(".rho/skills"), SkillOrigin::Project, &mut resolved);
        scan_directory(&project_dir.join("skills"), SkillOrigin::Project, &mut resolved);
    }
    resolved.sort_by(|left, right| left.metadata.name.cmp(&right.metadata.name));
    resolved
}

/// Full content of one skill by name in the resolved set.
///
/// Skills are read from their recorded file location, never interpreted or executed.
pub fn resolved_content(skills: &[ResolvedSkill], name: &str) -> Option<String> {
    let skill = skills.iter().find(|skill| skill.metadata.name == name)?;
    std::fs::read_to_string(&skill.metadata.location).ok()
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
