use super::parser::parse_agent_file;
use super::persona::{PersonaMetadata, parse_persona_file};
use super::types::{AgentOrigin, AgentResolutionPaths, ResolvedAgent};
use std::path::{Path, PathBuf};

pub fn resolved_agents(project_dir: Option<&Path>) -> Vec<ResolvedAgent> {
    let fallback = crate::config::dirs_fallback();
    let mut out = Vec::new();
    let paths = AgentResolutionPaths {
        project_dir,
        home_dir: Some(&fallback),
    };
    collect_agents(paths, &mut out);
    out
}

pub async fn resolved_agents_for_paths_async(
    project_dir: Option<PathBuf>,
    home_dir: Option<PathBuf>,
) -> Vec<ResolvedAgent> {
    tokio::task::spawn_blocking(move || {
        let (p, h) = (project_dir, home_dir);
        let mut out = Vec::new();
        collect_agents(
            AgentResolutionPaths {
                project_dir: p.as_deref(),
                home_dir: h.as_deref(),
            },
            &mut out,
        );
        out
    })
    .await
    .unwrap_or_default()
}

/// Personas from `~/.agents/personas` and the project's `.agents/personas`; a project persona
/// replaces a user persona with the same name.
pub fn resolved_personas(project_dir: Option<&Path>) -> Vec<PersonaMetadata> {
    let fallback = crate::config::dirs_fallback();
    resolved_personas_for_paths(AgentResolutionPaths {
        project_dir,
        home_dir: Some(&fallback),
    })
}

pub fn resolved_personas_for_paths(paths: AgentResolutionPaths<'_>) -> Vec<PersonaMetadata> {
    let mut resolved: Vec<PersonaMetadata> = Vec::new();
    for (dir, _) in search_dirs(paths, ".agents/personas", &[".agents/personas"]) {
        scan_recursive(&dir, &mut |path| {
            if let Some(persona) = parse_persona_file(path) {
                replace_or_push(&mut resolved, persona, |p| &p.name);
            }
        });
    }
    resolved.sort_by(|left, right| left.name.cmp(&right.name));
    resolved
}

fn search_dirs(
    paths: AgentResolutionPaths<'_>,
    home_subdir: &str,
    project_subdirs: &[&str],
) -> Vec<(PathBuf, AgentOrigin)> {
    let mut dirs = Vec::new();
    if let Some(home) = paths.home_dir {
        dirs.push((home.join(home_subdir), AgentOrigin::User));
    }
    if let Some(project) = paths.project_dir
        && paths.home_dir != Some(project)
    {
        dirs.extend(
            project_subdirs
                .iter()
                .map(|sub| (project.join(sub), AgentOrigin::Project)),
        );
    }
    dirs
}

fn collect_agents(paths: AgentResolutionPaths<'_>, resolved: &mut Vec<ResolvedAgent>) {
    for (dir, origin) in search_dirs(paths, ".agents/agents", &[".claude/agents", ".agents/agents"]) {
        scan_recursive(&dir, &mut |path| {
            if let Some(metadata) = parse_agent_file(path) {
                replace_or_push(resolved, ResolvedAgent { metadata, origin }, |a| &a.metadata.name);
            }
        });
    }
    resolved.sort_by(|left, right| left.metadata.name.cmp(&right.metadata.name));
}

fn replace_or_push<T>(resolved: &mut Vec<T>, item: T, name: impl Fn(&T) -> &String) {
    match resolved.iter().position(|existing| name(existing) == name(&item)) {
        Some(idx) => resolved[idx] = item,
        None => resolved.push(item),
    }
}

/// Visits every Markdown file under `dir` in sorted order.
fn scan_recursive(dir: &Path, visit: &mut dyn FnMut(&Path)) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    entries.sort();
    for entry in entries {
        if entry.is_dir() {
            scan_recursive(&entry, visit);
        } else if entry.extension().is_some_and(|ext| ext == "md") {
            visit(&entry);
        }
    }
}
