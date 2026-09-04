use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, Default)]
pub struct ContextDirs<'a> {
    pub config_dir: Option<&'a Path>,
    pub home_dir: Option<&'a Path>,
}

pub fn discover_instructions(base: &Path, dirs: ContextDirs<'_>) -> Vec<(String, String)> {
    let mut files = Vec::new();
    let mut seen = HashSet::new();

    // 1. Global / user-level instructions (broadest scope, loaded first)
    if let Some(home) = dirs.home_dir {
        load_candidate_instructions(&home.join(".agents"), &mut files, &mut seen);
    }

    // 2. Project-level instructions (committed base first, then workspace root)
    load_candidate_instructions(&base.join(".agents"), &mut files, &mut seen);
    load_candidate_instructions(base, &mut files, &mut seen);

    files
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
            files.push((file_path.display().to_string(), content.trim().to_string()));
        }
    }
}
