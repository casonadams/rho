use super::parser::parse_skill_file;
use super::types::{ResolvedSkill, SkillMetadata, SkillOrigin, SkillResolutionPaths};
use std::path::{Path, PathBuf};

/// Resolve every skill available to the session: declarative skills as `SKILL.md`
/// files under user directory `~/.agents/skills` and project skill directories
/// (`.agents/skills`, `skills`). Project skills replace user skills by name.
/// Skills carry readable content only and are never executed.
pub fn resolved_skills(project_dir: Option<&Path>) -> Vec<ResolvedSkill> {
    resolved_skills_with_home(project_dir, None)
}

pub async fn resolved_skills_async(project_dir: Option<&Path>) -> Vec<ResolvedSkill> {
    resolved_skills_with_home_async(project_dir, None).await
}

/// Resolve skills with an optional explicit home directory override, falling back to environment.
pub fn resolved_skills_with_home(project_dir: Option<&Path>, home_dir: Option<&Path>) -> Vec<ResolvedSkill> {
    let env_home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
        .map(PathBuf::from);
    let paths = SkillResolutionPaths {
        project_dir,
        home_dir: home_dir.or(env_home.as_deref()),
    };
    resolved_skills_for_paths(paths)
}

pub async fn resolved_skills_with_home_async(
    project_dir: Option<&Path>,
    home_dir: Option<&Path>,
) -> Vec<ResolvedSkill> {
    let project_owned = project_dir.map(Path::to_path_buf);
    let home_owned = home_dir.map(Path::to_path_buf);
    tokio::task::spawn_blocking(move || resolved_skills_with_home(project_owned.as_deref(), home_owned.as_deref()))
        .await
        .unwrap_or_default()
}

pub async fn resolved_skills_for_paths_async(
    project_dir: Option<PathBuf>,
    home_dir: Option<PathBuf>,
) -> Vec<ResolvedSkill> {
    tokio::task::spawn_blocking(move || {
        let paths = SkillResolutionPaths {
            project_dir: project_dir.as_deref(),
            home_dir: home_dir.as_deref(),
        };
        resolved_skills_for_paths(paths)
    })
    .await
    .unwrap_or_default()
}

/// Resolve skills with an explicit user home directory.
pub fn resolved_skills_for_paths(paths: SkillResolutionPaths<'_>) -> Vec<ResolvedSkill> {
    let mut resolved: Vec<ResolvedSkill> = Vec::new();

    if let Some(home_path) = paths.home_dir {
        scan_directory(&home_path.join(".agents/skills"), SkillOrigin::User, &mut resolved);
    }
    if let Some(project_dir) = paths.project_dir {
        if paths.home_dir != Some(project_dir) {
            scan_directory(&project_dir.join(".agents/skills"), SkillOrigin::Project, &mut resolved);
        }
        scan_directory(&project_dir.join("skills"), SkillOrigin::Project, &mut resolved);
    }
    resolved.sort_by(|left, right| left.metadata.name.cmp(&right.metadata.name));
    resolved
}

fn skill_file_for_entry(path: &Path) -> Option<PathBuf> {
    if path.is_dir() {
        Some(path.join("SKILL.md"))
    } else if path.extension().is_some_and(|ext| ext == "md") {
        Some(path.to_path_buf())
    } else {
        None
    }
}

fn process_skill_path(path: &Path, origin: SkillOrigin, resolved: &mut Vec<ResolvedSkill>) {
    let Some(skill_file) = skill_file_for_entry(path) else {
        return;
    };
    if skill_file.is_file()
        && let Some(metadata) = parse_skill_file(&skill_file)
    {
        upsert_by_name(resolved, origin, metadata);
    }
}

fn scan_directory(directory: &Path, origin: SkillOrigin, resolved: &mut Vec<ResolvedSkill>) {
    if !directory.is_dir() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    let mut paths: Vec<_> = entries.flatten().map(|entry| entry.path()).collect();
    paths.sort();
    for path in paths {
        process_skill_path(&path, origin, resolved);
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
