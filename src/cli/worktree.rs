//! CLI handler for `rho worktree [list|add|clear]`.

use crate::config::Config;
use crate::config::cli::WorktreeCommands;
use rho_engine::worktree::{
    clear_managed_worktrees, create_session_worktree, default_session_branch, list_managed_worktrees,
};

pub async fn handle_worktree(
    action: Option<WorktreeCommands>,
    config: &Config,
) -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    let base_dir = config.worktree.effective_base(&config.config_dir);

    match action.unwrap_or(WorktreeCommands::List) {
        WorktreeCommands::List => {
            let items = list_managed_worktrees(&cwd, &base_dir)
                .await
                .map_err(std::io::Error::other)?;
            if items.is_empty() {
                println!("No managed worktrees under {}.", base_dir.display());
                return Ok(());
            }
            for item in items {
                let branch = item.branch.as_deref().unwrap_or("(detached)");
                println!("{branch}\t{}", item.path.display());
            }
        }
        WorktreeCommands::Add { branch } => {
            let branch = branch
                .as_deref()
                .map(str::trim)
                .filter(|b| !b.is_empty())
                .map_or_else(default_session_branch, str::to_string);
            let created = create_session_worktree(&cwd, &base_dir, &branch, config.worktree.clean_source)
                .await
                .map_err(std::io::Error::other)?;
            println!("{}", created.summary());
        }
        WorktreeCommands::Clear { force } => {
            let report = clear_managed_worktrees(&cwd, &base_dir, force)
                .await
                .map_err(std::io::Error::other)?;
            if report.removed.is_empty() && report.skipped.is_empty() {
                println!("No managed worktrees to remove under {}.", base_dir.display());
                return Ok(());
            }
            for path in &report.removed {
                println!("Removed {}", path.display());
            }
            for (path, reason) in &report.skipped {
                eprintln!("Skipped {}: {reason}", path.display());
            }
        }
    }
    Ok(())
}
