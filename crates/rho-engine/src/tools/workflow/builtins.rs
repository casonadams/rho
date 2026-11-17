use super::shell::run_git;
use crate::tools::engine_tool::TurnToolContext;
use crate::tools::traversal::list_workspace_files;
use rho_harness_core::workflow::{default_max_scopes, plan_scopes, reviewable_dir_counts};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::Path;

const MAX_INVENTORY_FILES: usize = 50_000;

async fn git_ok(ctx: &TurnToolContext, cwd: &Path, args: &[&str]) -> Result<String, String> {
    let outcome = run_git(ctx, cwd, args).await?;
    if outcome.success() {
        Ok(outcome.stdout.trim().to_string())
    } else {
        Err(outcome.failure_summary(&format!("git {}", args.join(" "))))
    }
}

pub(crate) async fn repo_scopes(base_dir: &Path, with: &BTreeMap<String, String>) -> Result<Value, String> {
    let root = base_dir.to_path_buf();
    let files = tokio::task::spawn_blocking(move || list_workspace_files(&root, MAX_INVENTORY_FILES))
        .await
        .map_err(|e| format!("repository walk failed: {e}"))?;
    let counts = reviewable_dir_counts(&files);
    let reviewable: usize = counts.values().sum();
    let max_scopes = match with.get("max_scopes") {
        Some(raw) => raw
            .trim()
            .parse::<usize>()
            .map_err(|_| format!("max_scopes must be a positive integer (got '{raw}')"))?,
        None => default_max_scopes(reviewable),
    };
    Ok(json!({
        "items": plan_scopes(&counts, max_scopes),
        "total_files": files.len(),
        "reviewable_files": reviewable,
        "truncated": files.len() >= MAX_INVENTORY_FILES,
    }))
}

pub(crate) async fn git_commit(ctx: &TurnToolContext, cwd: &Path, message: &str) -> Result<Value, String> {
    if git_ok(ctx, cwd, &["status", "--porcelain"]).await?.is_empty() {
        return Ok(json!({ "committed": false }));
    }
    git_ok(ctx, cwd, &["add", "-A"]).await?;
    git_ok(ctx, cwd, &["commit", "--quiet", "-m", message]).await?;
    let sha = git_ok(ctx, cwd, &["rev-parse", "--short", "HEAD"]).await?;
    Ok(json!({ "committed": true, "sha": sha }))
}

pub(crate) async fn start_branch(ctx: &TurnToolContext, cwd: &Path, workflow: &str) -> Result<String, String> {
    let status = git_ok(ctx, cwd, &["status", "--porcelain"]).await?;
    if !status.is_empty() {
        return Err(format!(
            "workflow '{workflow}' runs on its own branch and needs a clean working tree; commit or stash these changes first:\n{status}"
        ));
    }
    let branch = format!("rho/{workflow}-{}", chrono::Local::now().format("%Y%m%d-%H%M%S"));
    git_ok(ctx, cwd, &["switch", "--quiet", "-c", &branch]).await?;
    Ok(branch)
}

pub(crate) async fn stash_changes(ctx: &TurnToolContext, cwd: &Path, message: &str) -> Result<bool, String> {
    if git_ok(ctx, cwd, &["status", "--porcelain"]).await?.is_empty() {
        return Ok(false);
    }
    git_ok(
        ctx,
        cwd,
        &["stash", "push", "--quiet", "--include-untracked", "-m", message],
    )
    .await?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(dir: &Path, args: &[&str]) {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    }

    fn repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "--quiet", "-b", "main"]);
        git(dir.path(), &["config", "user.email", "t@example.com"]);
        git(dir.path(), &["config", "user.name", "t"]);
        git(dir.path(), &["config", "commit.gpgsign", "false"]);
        std::fs::write(dir.path().join("a.rs"), "fn a() {}\n").unwrap();
        git(dir.path(), &["add", "-A"]);
        git(dir.path(), &["commit", "--quiet", "-m", "init"]);
        dir
    }

    #[tokio::test]
    async fn branch_commit_and_stash_round_trip() {
        let dir = repo();
        let ctx = TurnToolContext::default();
        let branch = start_branch(&ctx, dir.path(), "refine").await.unwrap();
        assert!(branch.starts_with("rho/refine-"));

        assert_eq!(
            git_commit(&ctx, dir.path(), "noop").await.unwrap(),
            json!({ "committed": false })
        );
        std::fs::write(dir.path().join("a.rs"), "fn a() { todo!() }\n").unwrap();
        let committed = git_commit(&ctx, dir.path(), "fix(review): round 1").await.unwrap();
        assert_eq!(committed["committed"], true);

        std::fs::write(dir.path().join("new.rs"), "x").unwrap();
        assert!(stash_changes(&ctx, dir.path(), "failed round").await.unwrap());
        assert!(!dir.path().join("new.rs").exists());
        assert!(!stash_changes(&ctx, dir.path(), "nothing").await.unwrap());
    }

    #[tokio::test]
    async fn run_logs_never_dirty_the_working_tree() {
        let dir = repo();
        super::super::report::RunLog::create(dir.path(), "refine").unwrap();
        let status = std::process::Command::new("git")
            .args(["-c", "core.excludesFile=/dev/null", "status", "--porcelain"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&status.stdout), "");
    }

    #[tokio::test]
    async fn branch_isolation_refuses_a_dirty_tree() {
        let dir = repo();
        std::fs::write(dir.path().join("dirty.rs"), "x").unwrap();
        let err = start_branch(&TurnToolContext::default(), dir.path(), "refine")
            .await
            .unwrap_err();
        assert!(err.contains("needs a clean working tree"), "{err}");
        assert!(err.contains("dirty.rs"), "{err}");
    }

    #[tokio::test]
    async fn repo_scopes_plans_from_reviewable_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/lib.rs"), "").unwrap();
        std::fs::write(dir.path().join("README.md"), "").unwrap();
        let value = repo_scopes(dir.path(), &BTreeMap::new()).await.unwrap();
        assert_eq!(value["reviewable_files"], 1);
        assert_eq!(value["items"][0]["paths"], json!(["."]));

        let mut with = BTreeMap::new();
        with.insert("max_scopes".to_string(), "x".to_string());
        assert!(repo_scopes(dir.path(), &with).await.unwrap_err().contains("max_scopes"));
    }
}
