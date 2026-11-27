use super::instructions::{CANDIDATE_FILES, rules_files};
use super::{ProjectContext, find_repo_root, transclusion};
use crate::paths::canonical_or_self;
use std::path::{Path, PathBuf};

pub const MAX_DYNAMIC_INSTRUCTION_FILES: usize = 10;
pub const MAX_DYNAMIC_INSTRUCTION_BYTES: usize = 64 * 1024;

pub async fn activate_path_instructions_async(ctx: &mut ProjectContext, path: &Path) {
    if ctx.no_context_files
        || ctx.dynamic_instructions_count >= MAX_DYNAMIC_INSTRUCTION_FILES
        || ctx.dynamic_instructions_bytes >= MAX_DYNAMIC_INSTRUCTION_BYTES
    {
        return;
    }
    let path = path.to_path_buf();
    let mut cloned_ctx = ctx.clone();
    let updated = tokio::task::spawn_blocking(move || {
        activate_path_instructions(&mut cloned_ctx, &path);
        cloned_ctx
    })
    .await
    .ok();
    if let Some(res) = updated {
        *ctx = res;
    }
}

fn target_dir_for(path: &Path) -> PathBuf {
    if path.is_dir() {
        path.to_path_buf()
    } else if let Some(parent) = path.parent() {
        parent.to_path_buf()
    } else {
        path.to_path_buf()
    }
}

fn canonicalize_best_effort(mut path: &Path) -> Option<PathBuf> {
    while !path.exists() {
        path = path.parent()?;
    }
    path.canonicalize().ok()
}

fn determine_walk_root<'a>(
    canonical_target: &Path,
    canonical_current: &'a Path,
    canonical_repo: Option<&'a Path>,
) -> Option<&'a Path> {
    if let Some(root) = canonical_repo
        && canonical_target.starts_with(root)
    {
        Some(root)
    } else if canonical_target.starts_with(canonical_current) {
        Some(canonical_current)
    } else {
        None
    }
}

fn walk_path_instructions(ctx: &mut ProjectContext, walk_root: &Path, canonical_target: &Path) {
    let Ok(rel) = canonical_target.strip_prefix(walk_root) else {
        return;
    };
    let mut curr = walk_root.to_path_buf();
    for component in rel.components() {
        curr.push(component);
        load_candidate_instructions(&curr.join(".agents"), ctx);
        load_candidate_instructions(&curr.join(".claude"), ctx);
        load_candidate_instructions(&curr, ctx);
    }
}

pub fn activate_path_instructions(ctx: &mut ProjectContext, path: &Path) {
    if ctx.no_context_files
        || ctx.dynamic_instructions_count >= MAX_DYNAMIC_INSTRUCTION_FILES
        || ctx.dynamic_instructions_bytes >= MAX_DYNAMIC_INSTRUCTION_BYTES
    {
        return;
    }
    let target = resolve_target_path(&ctx.current_dir, path);
    let target_dir = target_dir_for(&target);
    let Some(canonical_target) = canonicalize_best_effort(&target_dir) else {
        return;
    };
    let canonical_current = canonical_or_self(&ctx.current_dir);
    let repo_root = find_repo_root(&ctx.current_dir);
    let canonical_repo = repo_root.as_ref().and_then(|r| r.canonicalize().ok());
    let Some(walk_root) = determine_walk_root(&canonical_target, &canonical_current, canonical_repo.as_deref()) else {
        return;
    };
    walk_path_instructions(ctx, walk_root, &canonical_target);
}

fn resolve_target_path(current_dir: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    let cwd_target = current_dir.join(path);
    if cwd_target.exists() {
        return cwd_target;
    }
    if let Some(repo_root) = find_repo_root(current_dir) {
        let repo_target = repo_root.join(path);
        if repo_target.exists() {
            return repo_target;
        }
    }
    cwd_target
}

fn read_and_expand_instruction(file_path: &Path, fallback_dir: &Path) -> Option<String> {
    let content = std::fs::read_to_string(file_path).ok()?;
    let base_dir = file_path.parent().unwrap_or(fallback_dir);
    Some(transclusion::expand_transclusions_with_root(
        &content,
        base_dir,
        Some(file_path),
    ))
}

fn try_load_candidate(file_path: &Path, dir: &Path, ctx: &mut ProjectContext) -> bool {
    if !file_path.is_file() {
        return true;
    }
    let canonical = canonical_or_self(file_path);
    if !ctx.seen_instruction_files.insert(canonical) {
        return true;
    }
    let Some(expanded) = read_and_expand_instruction(file_path, dir) else {
        return true;
    };
    let remaining_bytes = MAX_DYNAMIC_INSTRUCTION_BYTES.saturating_sub(ctx.dynamic_instructions_bytes);
    if remaining_bytes == 0 {
        return false;
    }
    let trimmed = expanded.trim();
    let final_content = trimmed[..trimmed.floor_char_boundary(remaining_bytes)].to_string();
    ctx.dynamic_instructions_bytes += final_content.len();
    ctx.dynamic_instructions_count += 1;
    ctx.instruction_files
        .push((file_path.display().to_string(), final_content));
    true
}

fn load_candidate_instructions(dir: &Path, ctx: &mut ProjectContext) {
    if !dir.is_dir() {
        return;
    }
    let candidates = CANDIDATE_FILES
        .iter()
        .map(|name| dir.join(name))
        .chain(rules_files(dir));
    for file_path in candidates {
        if ctx.dynamic_instructions_count >= MAX_DYNAMIC_INSTRUCTION_FILES
            || ctx.dynamic_instructions_bytes >= MAX_DYNAMIC_INSTRUCTION_BYTES
            || !try_load_candidate(&file_path, dir, ctx)
        {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::context::discover_ancestry_instructions;
    use std::path::Path;

    #[test]
    fn activation_loads_the_same_candidate_files_as_discovery() {
        let dir = crate::test_support::temp_dir("activation-candidates");
        let sub = dir.join("sub");
        std::fs::create_dir_all(sub.join("rules")).unwrap();
        for name in CANDIDATE_FILES {
            std::fs::write(sub.join(name), format!("from {name}")).unwrap();
        }
        std::fs::write(sub.join("rules").join("style.md"), "rule").unwrap();

        let discovered: Vec<String> = discover_ancestry_instructions(&sub, Some(&dir))
            .into_iter()
            .map(|(path, _)| file_name(&path))
            .collect();
        let mut ctx = ProjectContext::for_tests(&dir);
        ctx.no_context_files = false;
        activate_path_instructions(&mut ctx, Path::new("sub/file.rs"));
        let activated: Vec<String> = ctx.instruction_files.iter().map(|(path, _)| file_name(path)).collect();

        assert_eq!(activated, discovered);
        assert!(activated.contains(&"CLAUDE.local.md".to_string()), "{activated:?}");
    }

    fn file_name(path: &str) -> String {
        Path::new(path).file_name().unwrap().to_string_lossy().into_owned()
    }
}
