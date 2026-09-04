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
        )?)
    } else if cli.r#continue {
        let cwd = std::env::current_dir()?;
        Ok(SessionManager::last_session_for_cwd(&config.sessions_dir, &cwd)?)
    } else {
        Ok(cli.resume.clone())
    }
}

pub async fn export_session(
    export_path: &str,
    resume_target: Option<String>,
    config: &Config,
) -> Result<(), Box<dyn std::error::Error>> {
    let resume_target_id = match resume_target {
        Some(id) => id,
        None => {
            let cwd = std::env::current_dir()?;
            SessionManager::last_session_for_cwd(&config.sessions_dir, &cwd)?
                .ok_or_else(|| AppError::Session("no session found to export".to_string()))?
        }
    };
    let session_manager = SessionManager::new(&config.sessions_dir, Some(&resume_target_id))?;
    let tree = session_manager.load_tree().await?;
    let path = PathBuf::from(export_path);
    let content = if path.extension().and_then(|ext| ext.to_str()) == Some("html") {
        rho_harness_core::session::export::render_html(&tree, &resume_target_id)
    } else {
        rho_harness_core::session::export::render_markdown(&tree, &resume_target_id)
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, content)?;
    println!("Exported session {} to {}", resume_target_id, path.display());
    Ok(())
}
