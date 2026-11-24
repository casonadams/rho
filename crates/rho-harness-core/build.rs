use std::path::{Path, PathBuf};
use std::process::Command;

fn find_workspace_root(start: &Path) -> PathBuf {
    let mut dir = start;
    loop {
        if dir.join(".git").exists() {
            return dir.to_path_buf();
        }
        match dir.parent() {
            Some(parent) => dir = parent,
            None => return start.to_path_buf(),
        }
    }
}

fn resolve_git_dir(workspace: &Path) -> Option<PathBuf> {
    let dot_git = workspace.join(".git");
    if dot_git.is_dir() {
        return Some(dot_git);
    }
    let content = std::fs::read_to_string(&dot_git).ok()?;
    let raw = content.trim().strip_prefix("gitdir:")?.trim();
    let path = Path::new(raw);
    Some(if path.is_absolute() {
        path.to_path_buf()
    } else {
        workspace.join(path)
    })
}

fn emit_git_rerun_hints(git_dir: &Path) {
    let head = git_dir.join("HEAD");
    if head.exists() {
        println!("cargo:rerun-if-changed={}", head.display());
    }
    let index = git_dir.join("index");
    if index.exists() {
        println!("cargo:rerun-if-changed={}", index.display());
    }
    if let Ok(content) = std::fs::read_to_string(&head)
        && let Some(ref_rel) = content.trim().strip_prefix("ref:")
    {
        let ref_path = git_dir.join(ref_rel.trim());
        if ref_path.exists() {
            println!("cargo:rerun-if-changed={}", ref_path.display());
        }
    }
}

fn git_stdout(workspace: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).current_dir(workspace).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn resolve_git_sha(workspace: &Path) -> String {
    let Some(sha) = git_stdout(workspace, &["rev-parse", "--short", "HEAD"]) else {
        return String::new();
    };
    let dirty = git_stdout(workspace, &["status", "--porcelain"]).is_some();
    if dirty { format!("{sha}-dirty") } else { sha }
}

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    let workspace = find_workspace_root(&manifest_dir);
    if let Some(git_dir) = resolve_git_dir(&workspace) {
        emit_git_rerun_hints(&git_dir);
    }

    let git_sha = resolve_git_sha(&workspace);
    let profile = std::env::var("PROFILE").unwrap_or_else(|_| "unknown".to_string());
    println!("cargo:rustc-env=RHO_GIT_SHA={git_sha}");
    println!("cargo:rustc-env=RHO_BUILD_PROFILE={profile}");
}
