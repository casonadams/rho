use super::git;
use std::path::Path;

const EXCLUDE_MARKER: &str = "# rho worktrees: ignored entries shared into agent worktrees";

/// Top-level ignored entries of `root` (build output, dependencies, `.env`), from `git status`.
async fn ignored_top_level(root: &Path) -> Result<Vec<String>, String> {
    let out = git::run(
        root,
        &[
            "ls-files",
            "--others",
            "--ignored",
            "--exclude-standard",
            "--directory",
            "-z",
        ],
        &[],
    )
    .await?;
    let mut names: Vec<String> = git::nul_fields(&out)
        .into_iter()
        .map(|entry| entry.trim_end_matches('/').to_string())
        .filter(|name| !name.is_empty() && !name.contains('/') && name != ".git")
        .collect();
    names.sort();
    names.dedup();
    Ok(names)
}

/// Adds `/<name>` lines to the repository's shared `info/exclude`, so the symlinks that stand in for
/// ignored directories are ignored too (a `dir/` pattern does not match a symlink).
async fn exclude(common_dir: &Path, names: &[String]) -> Result<(), String> {
    let path = common_dir.join("info").join("exclude");
    let current = tokio::fs::read_to_string(&path).await.unwrap_or_default();
    let missing: Vec<String> = names
        .iter()
        .map(|name| format!("/{name}"))
        .filter(|line| !current.lines().any(|l| l.trim() == line))
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    let mut text = current;
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    if !text.contains(EXCLUDE_MARKER) {
        text.push_str(EXCLUDE_MARKER);
        text.push('\n');
    }
    for line in missing {
        text.push_str(&line);
        text.push('\n');
    }
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|e| e.to_string())?;
    }
    tokio::fs::write(&path, text)
        .await
        .map_err(|e| format!("could not update {}: {e}", path.display()))
}

/// Symlinks `source`'s top-level ignored entries into `worktree`, so agents reuse build caches,
/// installed dependencies, and local env files instead of starting cold.
pub(crate) async fn share_ignored(source: &Path, worktree: &Path, common_dir: &Path) -> Result<(), String> {
    let names = ignored_top_level(source).await?;
    let mut excluded = names.clone();
    excluded.push(".rho/artifacts/".to_string());
    exclude(common_dir, &excluded).await?;
    for name in &names {
        let target = std::fs::canonicalize(source.join(name)).unwrap_or_else(|_| source.join(name));
        let link = worktree.join(name);
        if link.symlink_metadata().is_ok() {
            continue;
        }
        symlink(&target, &link).map_err(|e| format!("could not share {name} into the worktree: {e}"))?;
    }
    Ok(())
}

#[cfg(unix)]
pub(super) fn symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
pub(super) fn symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    if target.is_dir() {
        std::os::windows::fs::symlink_dir(target, link)
    } else {
        std::os::windows::fs::symlink_file(target, link)
    }
}
