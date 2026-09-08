//! Session resolution and export utilities for the CLI.

use crate::config::Config;
use crate::config::cli::Cli;
use rho_harness_core::error::AppError;
use rho_harness_core::session::SessionManager;
use std::path::PathBuf;

pub fn resolve_resume_target(cli: &Cli, config: &Config) -> Result<Option<String>, Box<dyn std::error::Error>> {
    if cli.resume_picker {
        Ok(crate::ui::interactive::session_picker::prompt_session_picker(
            &config.sessions_dir,
            &crate::ui::theme::detect(),
        )?)
    } else if cli.r#continue {
        let cwd = std::env::current_dir()?;
        Ok(SessionManager::last_session_for_cwd(&config.sessions_dir, &cwd)?)
    } else {
        Ok(cli.resume.clone())
    }
}

async fn resolve_export_target_id(
    resume_target: Option<String>,
    sessions_dir: &std::path::Path,
) -> Result<String, Box<dyn std::error::Error>> {
    if let Some(id) = resume_target {
        return Ok(id);
    }
    let cwd = std::env::current_dir()?;
    SessionManager::last_session_for_cwd_async(sessions_dir, &cwd)
        .await?
        .ok_or_else(|| AppError::Session("no session found to export".to_string()).into())
}

fn render_export_content(
    tree: &rho_harness_core::session::tree::SessionTree,
    id: &str,
    path: &std::path::Path,
) -> String {
    if path.extension().and_then(|ext| ext.to_str()) == Some("html") {
        rho_harness_core::session::export::render_html(tree, id)
    } else {
        rho_harness_core::session::export::render_markdown(tree, id)
    }
}

async fn write_export_file(path: &std::path::Path, content: &str) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(path, content).await?;
    Ok(())
}

pub async fn export_session(
    export_path: &str,
    resume_target: Option<String>,
    config: &Config,
) -> Result<(), Box<dyn std::error::Error>> {
    let target_id = resolve_export_target_id(resume_target, &config.sessions_dir).await?;
    let session_manager = SessionManager::new_async(&config.sessions_dir, Some(&target_id)).await?;
    let tree = session_manager.load_tree().await?;
    let path = PathBuf::from(export_path);
    let content = render_export_content(&tree, &target_id, &path);
    write_export_file(&path, &content).await?;
    println!("Exported session {} to {}", target_id, path.display());
    Ok(())
}
