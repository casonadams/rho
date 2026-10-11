use crate::lsp::LspProcess;
use crate::lsp::client::LspClient;
use rho_harness_core::error::AppError;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct LspManager {
    base_dir: PathBuf,
    client: tokio::sync::Mutex<Option<Arc<LspClient>>>,
}

impl LspManager {
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self::for_root(base_dir.into())
    }

    pub fn with_client(base_dir: impl Into<PathBuf>, client: Arc<LspClient>) -> Self {
        let root = base_dir.into();
        Self {
            base_dir: root,
            client: tokio::sync::Mutex::new(Some(client)),
        }
    }

    fn for_root(root: PathBuf) -> Self {
        Self {
            base_dir: root,
            client: tokio::sync::Mutex::new(None),
        }
    }

    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    pub fn auto_detect_command(&self) -> Option<(&'static str, &'static [&'static str])> {
        LspProcess::auto_detect_command(&self.base_dir)
    }

    pub async fn get_client(&self) -> Result<Arc<LspClient>, AppError> {
        let mut lock = self.client.lock().await;
        if let Some(existing) = &*lock
            && existing.is_connected()
        {
            return Ok(Arc::clone(existing));
        }
        let client = Arc::new(self.spawn_server().await?);
        client.initialize().await?;
        *lock = Some(Arc::clone(&client));
        Ok(client)
    }

    async fn spawn_server(&self) -> Result<LspClient, AppError> {
        let (server, args) = self
            .auto_detect_command()
            .ok_or_else(|| AppError::Tool("No standard LSP server auto-detected for workspace root".to_string()))?;

        let mut child = tokio::process::Command::new(server)
            .args(args)
            .current_dir(&self.base_dir)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    AppError::Tool(format!(
                        "LSP server '{server}' not found on PATH. Please install it to enable semantic analysis for this workspace."
                    ))
                } else {
                    AppError::Tool(format!("failed to spawn LSP server '{server}': {e}"))
                }
            })?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| AppError::Tool("failed to capture stdin of LSP child process".to_string()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| AppError::Tool("failed to capture stdout of LSP child process".to_string()))?;

        Ok(LspClient::new(stdout, stdin, self.base_dir.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manager_auto_detect_cargo() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("Cargo.toml"), "[package]").unwrap();
        let mgr = LspManager::new(temp.path());
        let (cmd, args) = mgr.auto_detect_command().expect("detected");
        assert_eq!(cmd, "rust-analyzer");
        assert!(args.is_empty());
    }

    #[test]
    fn test_manager_auto_detect_package_json() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("package.json"), "{}").unwrap();
        let mgr = LspManager::new(temp.path());
        let (cmd, args) = mgr.auto_detect_command().expect("detected");
        assert_eq!(cmd, "typescript-language-server");
        assert_eq!(args, &["--stdio"]);
    }

    #[test]
    fn test_manager_auto_detect_go() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("go.mod"), "module example").unwrap();
        let mgr = LspManager::new(temp.path());
        let (cmd, _) = mgr.auto_detect_command().expect("detected");
        assert_eq!(cmd, "gopls");
    }

    #[test]
    fn test_manager_auto_detect_python() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("pyproject.toml"), "").unwrap();
        let mgr = LspManager::new(temp.path());
        let (cmd, args) = mgr.auto_detect_command().expect("detected");
        assert_eq!(cmd, "pyright-langserver");
        assert_eq!(args, &["--stdio"]);
    }

    #[test]
    fn test_manager_auto_detect_none() {
        let temp = tempfile::tempdir().unwrap();
        let mgr = LspManager::new(temp.path());
        assert!(mgr.auto_detect_command().is_none());
    }

    #[tokio::test]
    async fn test_manager_with_injected_client() {
        let temp = tempfile::tempdir().unwrap();
        let (r, _) = tokio::io::duplex(64);
        let (_, w) = tokio::io::duplex(64);
        let client = Arc::new(LspClient::new(r, w, temp.path().to_path_buf()));
        let mgr = LspManager::with_client(temp.path(), client.clone());
        let fetched = mgr.get_client().await.expect("got client");
        assert!(Arc::ptr_eq(&client, &fetched));
    }
}
