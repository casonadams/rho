use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, Default)]
pub struct ContextDirs<'a> {
    pub config_dir: Option<&'a Path>,
    pub home_dir: Option<&'a Path>,
    pub system_prompt: Option<&'a str>,
    pub append_system_prompt: Option<&'a str>,
    pub no_context_files: bool,
}

pub fn discover_instructions(base: &Path, dirs: ContextDirs<'_>) -> Vec<(String, String)> {
    discover_instructions_with_seen(base, dirs).0
}

pub fn discover_instructions_with_seen(
    base: &Path,
    dirs: ContextDirs<'_>,
) -> (Vec<(String, String)>, HashSet<PathBuf>) {
    let mut files = Vec::new();
    let mut seen = HashSet::new();

    if dirs.no_context_files {
        return (files, seen);
    }

    if let Some(home) = dirs.home_dir {
        load_candidate_instructions(&home.join(".agents"), &mut files, &mut seen);
    }

    let repo_root = find_repo_root(base);
    let dirs = match repo_root.as_deref() {
        Some(root) => ancestry_chain(base, root),
        None => vec![base.to_path_buf()],
    };
    load_ancestry_instructions(&dirs, &mut files, &mut seen);

    (files, seen)
}

pub async fn discover_instructions_with_seen_async(
    base: &Path,
    dirs: ContextDirs<'_>,
) -> (Vec<(String, String)>, HashSet<PathBuf>) {
    let base_owned = base.to_path_buf();
    let home_owned = dirs.home_dir.map(Path::to_path_buf);
    let no_context = dirs.no_context_files;
    tokio::task::spawn_blocking(move || {
        discover_instructions_with_seen(
            &base_owned,
            ContextDirs {
                home_dir: home_owned.as_deref(),
                no_context_files: no_context,
                ..Default::default()
            },
        )
    })
    .await
    .unwrap_or_else(|_| (Vec::new(), HashSet::new()))
}

pub fn discover_ancestry_instructions(base: &Path, repo_root: Option<&Path>) -> Vec<(String, String)> {
    let mut files = Vec::new();
    let mut seen = HashSet::new();
    let dirs = match repo_root {
        Some(root) => ancestry_chain(base, root),
        None => vec![base.to_path_buf()],
    };
    load_ancestry_instructions(&dirs, &mut files, &mut seen);
    files
}

pub fn find_repo_root(base: &Path) -> Option<PathBuf> {
    let absolute = if base.is_absolute() {
        base.to_path_buf()
    } else {
        std::env::current_dir().ok()?.join(base)
    };
    let mut curr = Some(absolute.as_path());
    while let Some(dir) = curr {
        let git = dir.join(".git");
        if git.is_dir() || git.is_file() {
            return Some(dir.to_path_buf());
        }
        curr = dir.parent();
    }
    None
}

fn load_ancestry_instructions(dirs: &[PathBuf], files: &mut Vec<(String, String)>, seen: &mut HashSet<PathBuf>) {
    for dir in dirs {
        load_candidate_instructions(&dir.join(".agents"), files, seen);
        load_candidate_instructions(dir, files, seen);
    }
}

fn ancestry_chain(base: &Path, root: &Path) -> Vec<PathBuf> {
    let canonical_base = base.canonicalize().unwrap_or_else(|_| base.to_path_buf());
    let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());

    if let Ok(rel) = canonical_base.strip_prefix(&canonical_root) {
        let mut chain = vec![root.to_path_buf()];
        let mut curr = root.to_path_buf();
        for component in rel.components() {
            curr.push(component);
            chain.push(curr.clone());
        }
        chain
    } else {
        vec![base.to_path_buf()]
    }
}

fn load_candidate_instructions(dir: &Path, files: &mut Vec<(String, String)>, seen: &mut HashSet<PathBuf>) {
    if !dir.exists() || !dir.is_dir() {
        return;
    }
    let candidates = ["AGENTS.md", "CLAUDE.md", ".cursorrules"];
    for filename in candidates {
        let file_path = dir.join(filename);
        if !file_path.is_file() {
            continue;
        }
        let canonical = file_path.canonicalize().unwrap_or_else(|_| file_path.clone());
        if seen.insert(canonical)
            && let Ok(content) = std::fs::read_to_string(&file_path)
        {
            let base_dir = file_path.parent().unwrap_or(dir);
            let expanded = super::transclusion::expand_transclusions_with_root(&content, base_dir, Some(&file_path));
            files.push((file_path.display().to_string(), expanded.trim().to_string()));
        }
    }
}
