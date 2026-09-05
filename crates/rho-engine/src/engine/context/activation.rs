use super::{ProjectContext, find_repo_root, transclusion};
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

pub fn activate_path_instructions(ctx: &mut ProjectContext, path: &Path) {
    if ctx.no_context_files {
        return;
    }
    if ctx.dynamic_instructions_count >= MAX_DYNAMIC_INSTRUCTION_FILES
        || ctx.dynamic_instructions_bytes >= MAX_DYNAMIC_INSTRUCTION_BYTES
    {
        return;
    }
    let target = resolve_target_path(&ctx.current_dir, path);
    let target_dir = if target.is_dir() {
        target.clone()
    } else if let Some(parent) = target.parent() {
        parent.to_path_buf()
    } else {
        target.clone()
    };

    let canonical_target = match target_dir.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            let mut existing = target_dir.as_path();
            while !existing.exists() {
                match existing.parent() {
                    Some(parent) => existing = parent,
                    None => break,
                }
            }
            match existing.canonicalize() {
                Ok(p) => p,
                Err(_) => return,
            }
        }
    };

    let canonical_current = ctx
        .current_dir
        .canonicalize()
        .unwrap_or_else(|_| ctx.current_dir.clone());
    let repo_root = find_repo_root(&ctx.current_dir);
    let canonical_repo = repo_root.as_ref().and_then(|r| r.canonicalize().ok());

    let walk_root = if let Some(ref root) = canonical_repo
        && canonical_target.starts_with(root)
    {
        root
    } else if canonical_target.starts_with(&canonical_current) {
        &canonical_current
    } else {
        return;
    };

    let Ok(rel) = canonical_target.strip_prefix(walk_root) else {
        return;
    };

    let mut curr = walk_root.clone();
    for component in rel.components() {
        curr.push(component);
        load_candidate_instructions(&curr.join(".agents"), ctx);
        load_candidate_instructions(&curr, ctx);
    }
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

fn load_candidate_instructions(dir: &Path, ctx: &mut ProjectContext) {
    if !dir.exists() || !dir.is_dir() {
        return;
    }
    let candidates = ["AGENTS.md", "CLAUDE.md", ".cursorrules"];
    for filename in candidates {
        if ctx.dynamic_instructions_count >= MAX_DYNAMIC_INSTRUCTION_FILES
            || ctx.dynamic_instructions_bytes >= MAX_DYNAMIC_INSTRUCTION_BYTES
        {
            return;
        }
        let file_path = dir.join(filename);
        if !file_path.is_file() {
            continue;
        }
        let canonical = file_path.canonicalize().unwrap_or_else(|_| file_path.clone());
        if !ctx.seen_instruction_files.insert(canonical.clone()) {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&file_path) else {
            continue;
        };
        let base_dir = file_path.parent().unwrap_or(dir);
        let expanded = transclusion::expand_transclusions_with_root(&content, base_dir, Some(&file_path));
        let trimmed = expanded.trim();
        let remaining_bytes = MAX_DYNAMIC_INSTRUCTION_BYTES.saturating_sub(ctx.dynamic_instructions_bytes);
        if remaining_bytes == 0 {
            return;
        }
        let final_content = if trimmed.len() > remaining_bytes {
            truncate_to_char_boundary(trimmed, remaining_bytes)
        } else {
            trimmed.to_string()
        };
        ctx.dynamic_instructions_bytes += final_content.len();
        ctx.dynamic_instructions_count += 1;
        ctx.instruction_files
            .push((file_path.display().to_string(), final_content));
    }
}

fn truncate_to_char_boundary(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut boundary = max_bytes;
    while boundary > 0 && !s.is_char_boundary(boundary) {
        boundary -= 1;
    }
    s[..boundary].to_string()
}
