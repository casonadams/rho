use super::parser::parse_agent_file;
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

pub fn resolved_agents_for_paths(paths: AgentResolutionPaths<'_>) -> Vec<ResolvedAgent> {
    let mut resolved: Vec<ResolvedAgent> = Vec::new();
    collect_agents(paths, &mut resolved);
    resolved
}

fn collect_agents(paths: AgentResolutionPaths<'_>, resolved: &mut Vec<ResolvedAgent>) {
    let mut search_dirs = Vec::new();
    if let Some(home) = paths.home_dir {
        search_dirs.push((home.join(".claude/agents"), AgentOrigin::User));
        search_dirs.push((home.join(".agents/agents"), AgentOrigin::User));
    }
    if let Some(project) = paths.project_dir
        && paths.home_dir != Some(project)
    {
        search_dirs.push((project.join(".claude/agents"), AgentOrigin::Project));
        search_dirs.push((project.join(".agents/agents"), AgentOrigin::Project));
    }

    for (dir, origin) in search_dirs {
        scan_agent_directory_recursive(&dir, origin, resolved);
    }

    resolved.sort_by(|left, right| left.metadata.name.cmp(&right.metadata.name));
}

fn scan_agent_directory_recursive(dir: &Path, origin: AgentOrigin, resolved: &mut Vec<ResolvedAgent>) {
    if !dir.is_dir() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    entries.sort();
    for entry in entries {
        if entry.is_dir() {
            scan_agent_directory_recursive(&entry, origin, resolved);
        } else if entry.extension().is_some_and(|ext| ext == "md")
            && let Some(meta) = parse_agent_file(&entry)
        {
            let name = &meta.name;
            if let Some(idx) = resolved.iter().position(|a| &a.metadata.name == name) {
                resolved[idx] = ResolvedAgent { metadata: meta, origin };
            } else {
                resolved.push(ResolvedAgent { metadata: meta, origin });
            }
        }
    }
}
