//! Session resolution and export utilities for the CLI.

use crate::config::Config;
use crate::config::cli::Cli;
use rho_harness_core::error::AppError;
use rho_harness_core::session::SessionManager;
use rho_harness_core::session::export::{ExportFormat, write_export};
use std::path::PathBuf;

#[cfg(feature = "ui")]
fn resolve_picker_target(config: &Config) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let theme = crate::ui::theme::detect_with_config(&config.ui);
    Ok(crate::ui::interactive::session_picker::prompt_session_picker(
        &config.sessions_dir,
        &theme,
    )?)
}

#[cfg(not(feature = "ui"))]
fn resolve_picker_target(_config: &Config) -> Result<Option<String>, Box<dyn std::error::Error>> {
    Err(Box::new(AppError::Session(
        "the session picker is unavailable in headless mode (compiled without 'ui' feature); pass --resume <id> or --continue"
            .to_string(),
    )))
}

fn resolve_continue_target(sessions_dir: &std::path::Path) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    Ok(SessionManager::last_session_for_cwd(sessions_dir, &cwd)?)
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum SessionCwdRestore {
    Relocate(PathBuf),
    Missing(PathBuf),
    Unchanged,
}

pub(crate) fn resolve_session_cwd_to_restore(sessions_dir: &std::path::Path, session_id: &str) -> SessionCwdRestore {
    let Ok(Some(saved)) = SessionManager::cwd_for_session(sessions_dir, session_id) else {
        return SessionCwdRestore::Unchanged;
    };
    if saved.is_dir() {
        SessionCwdRestore::Relocate(saved)
    } else {
        SessionCwdRestore::Missing(saved)
    }
}

pub fn restore_session_cwd(sessions_dir: &std::path::Path, session_id: &str) {
    match resolve_session_cwd_to_restore(sessions_dir, session_id) {
        SessionCwdRestore::Relocate(saved) => {
            if let Err(err) = std::env::set_current_dir(&saved) {
                eprintln!("Warning: could not restore workspace {}: {err}", saved.display());
            }
        }
        SessionCwdRestore::Missing(saved) => {
            eprintln!(
                "Note: previous workspace {} no longer exists; staying in current directory.",
                saved.display()
            );
        }
        SessionCwdRestore::Unchanged => {}
    }
}

pub fn resolve_resume_target(cli: &Cli, config: &Config) -> Result<Option<String>, Box<dyn std::error::Error>> {
    if cli.resume_picker {
        resolve_picker_target(config)
    } else if cli.r#continue {
        resolve_continue_target(&config.sessions_dir)
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

pub async fn export_session(
    export_path: &str,
    resume_target: Option<String>,
    config: &Config,
) -> Result<(), Box<dyn std::error::Error>> {
    let target_id = resolve_export_target_id(resume_target, &config.sessions_dir).await?;
    let session_manager = SessionManager::new_async(&config.sessions_dir, Some(&target_id)).await?;
    let tree = session_manager.load_tree().await?;
    let path = PathBuf::from(export_path);
    let format = ExportFormat::from_extension(path.extension().and_then(|ext| ext.to_str()).unwrap_or(""));
    write_export(&path, &tree, &target_id, format).await?;
    println!("Exported session {} to {}", target_id, path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn test_resolve_resume_target_explicit_and_none() {
        let cli = Cli::try_parse_from(["rho", "--resume", "session-42"]).unwrap();
        let config = Config::default();
        let target = resolve_resume_target(&cli, &config).unwrap();
        assert_eq!(target.as_deref(), Some("session-42"));

        let cli = Cli::try_parse_from(["rho"]).unwrap();
        let target = resolve_resume_target(&cli, &config).unwrap();
        assert_eq!(target, None);
    }

    #[test]
    fn test_resolve_resume_target_continue() {
        let temp = tempfile::tempdir().unwrap();
        let config = Config {
            sessions_dir: temp.path().to_path_buf(),
            ..Default::default()
        };

        let cli = Cli::try_parse_from(["rho", "--continue"]).unwrap();
        let target = resolve_resume_target(&cli, &config).unwrap();
        assert_eq!(target, None);

        let cwd = std::env::current_dir().unwrap();
        SessionManager::record_session_for_cwd(&config.sessions_dir, &cwd, "sess-cwd-1").unwrap();
        let target = resolve_resume_target(&cli, &config).unwrap();
        assert_eq!(target.as_deref(), Some("sess-cwd-1"));
    }

    #[test]
    fn test_resolve_resume_target_picker_empty() {
        let temp = tempfile::tempdir().unwrap();
        let config = Config {
            sessions_dir: temp.path().to_path_buf(),
            ..Default::default()
        };

        let cli = Cli::try_parse_from(["rho", "--resume-picker"]).unwrap();
        let target = resolve_resume_target(&cli, &config).unwrap();
        assert_eq!(target, None);
    }

    #[tokio::test]
    async fn test_resolve_export_target_id() {
        let temp = tempfile::tempdir().unwrap();
        let sessions_dir = temp.path();

        let target = resolve_export_target_id(Some("explicit-id".to_string()), sessions_dir)
            .await
            .unwrap();
        assert_eq!(target, "explicit-id");

        let err = resolve_export_target_id(None, sessions_dir).await.unwrap_err();
        assert!(err.to_string().contains("no session found to export"));

        let cwd = std::env::current_dir().unwrap();
        SessionManager::record_session_for_cwd(sessions_dir, &cwd, "sess-auto").unwrap();
        let target = resolve_export_target_id(None, sessions_dir).await.unwrap();
        assert_eq!(target, "sess-auto");
    }

    #[tokio::test]
    async fn test_write_export_contains_session_id() {
        let temp = tempfile::tempdir().unwrap();
        let store = SessionManager::new(temp.path(), None).unwrap();
        let tree = store.load_tree().await.unwrap();

        let md = temp.path().join("export.md");
        write_export(&md, &tree, &store.session_id, ExportFormat::Markdown)
            .await
            .unwrap();
        assert!(
            tokio::fs::read_to_string(&md)
                .await
                .unwrap()
                .contains(&store.session_id)
        );

        let html = temp.path().join("export.html");
        write_export(&html, &tree, &store.session_id, ExportFormat::Html)
            .await
            .unwrap();
        assert!(
            tokio::fs::read_to_string(&html)
                .await
                .unwrap()
                .contains(&store.session_id)
        );
    }

    #[tokio::test]
    async fn test_write_export_file() {
        let temp = tempfile::tempdir().unwrap();
        let file_path = temp.path().join("nested").join("sub").join("output.txt");
        let tree = rho_harness_core::session::tree::SessionTree::new();
        write_export(&file_path, &tree, "sess", ExportFormat::Markdown)
            .await
            .unwrap();
        let read_back = tokio::fs::read_to_string(&file_path).await.unwrap();
        assert_eq!(
            read_back,
            rho_harness_core::session::export::render_markdown(&tree, "sess")
        );
    }

    #[tokio::test]
    async fn test_export_session_end_to_end() {
        let temp = tempfile::tempdir().unwrap();
        let config = Config {
            sessions_dir: temp.path().join("sessions"),
            ..Default::default()
        };
        let store = SessionManager::new(&config.sessions_dir, None).unwrap();
        let session_id = store.session_id.clone();
        drop(store);

        let export_md = temp.path().join("out").join("exported.md");
        export_session(export_md.to_str().unwrap(), Some(session_id.clone()), &config)
            .await
            .unwrap();
        assert!(export_md.exists());
        let content = tokio::fs::read_to_string(&export_md).await.unwrap();
        assert!(content.contains(&session_id));
    }

    #[test]
    fn resolve_session_cwd_to_restore_handles_existing_missing_and_unknown() {
        let dir = rho_test_support::TestDir::new();
        let sessions_dir = dir.join("sessions");
        let worktree = dir.join("wt");
        std::fs::create_dir_all(&worktree).unwrap();

        assert_eq!(
            resolve_session_cwd_to_restore(&sessions_dir, "missing-sess"),
            SessionCwdRestore::Unchanged
        );

        SessionManager::record_session_for_cwd(&sessions_dir, &worktree, "sess-wt").unwrap();
        let canonical_wt = std::fs::canonicalize(&worktree).unwrap();
        assert_eq!(
            resolve_session_cwd_to_restore(&sessions_dir, "sess-wt"),
            SessionCwdRestore::Relocate(canonical_wt.clone())
        );

        std::fs::remove_dir_all(&worktree).unwrap();
        assert_eq!(
            resolve_session_cwd_to_restore(&sessions_dir, "sess-wt"),
            SessionCwdRestore::Missing(canonical_wt)
        );
    }
}
