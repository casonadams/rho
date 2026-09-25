//! Session resolution and export utilities for the CLI.

use crate::config::Config;
use crate::config::cli::Cli;
use rho_harness_core::error::AppError;
use rho_harness_core::session::SessionManager;
use std::path::PathBuf;

fn resolve_picker_target(config: &Config) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let theme = crate::ui::theme::detect_with_config(&config.ui);
    Ok(crate::ui::interactive::session_picker::prompt_session_picker(
        &config.sessions_dir,
        &theme,
    )?)
}

fn resolve_continue_target(sessions_dir: &std::path::Path) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    Ok(SessionManager::last_session_for_cwd(sessions_dir, &cwd)?)
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
    async fn test_render_export_content() {
        let temp = tempfile::tempdir().unwrap();
        let store = SessionManager::new(temp.path(), None).unwrap();
        let tree = store.load_tree().await.unwrap();

        let md = render_export_content(&tree, &store.session_id, std::path::Path::new("export.md"));
        assert!(md.contains(&store.session_id));

        let html = render_export_content(&tree, &store.session_id, std::path::Path::new("export.html"));
        assert!(html.contains(&store.session_id));
    }

    #[tokio::test]
    async fn test_write_export_file() {
        let temp = tempfile::tempdir().unwrap();
        let file_path = temp.path().join("nested").join("sub").join("output.txt");
        write_export_file(&file_path, "hello world").await.unwrap();
        let read_back = tokio::fs::read_to_string(&file_path).await.unwrap();
        assert_eq!(read_back, "hello world");
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
}
