use rho_harness_core::error::{AppError, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const TRUSTED_FILE_NAME: &str = "trusted_workspaces.json";
const APPROVED_SERVERS_FILE_NAME: &str = "mcp_approved_servers.json";

#[derive(Debug, Default, Serialize, Deserialize)]
struct TrustedWorkspaces {
    #[serde(default)]
    trusted: BTreeSet<PathBuf>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct ApprovedServers {
    #[serde(default)]
    approved: std::collections::BTreeMap<String, String>,
}

fn canonical_path(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

fn trust_file_path(config_dir: &Path) -> PathBuf {
    config_dir.join(TRUSTED_FILE_NAME)
}

fn load_trusted(config_dir: &Path) -> TrustedWorkspaces {
    let path = trust_file_path(config_dir);
    if !path.exists() {
        return TrustedWorkspaces::default();
    }
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str::<TrustedWorkspaces>(&s).ok())
        .unwrap_or_default()
}

fn save_trusted(config_dir: &Path, data: &TrustedWorkspaces) -> Result<()> {
    let path = trust_file_path(config_dir);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(data)
        .map_err(|e| AppError::Config(format!("Failed to serialize trusted workspaces: {e}")))?;
    std::fs::write(&path, content)
        .map_err(|e| AppError::Config(format!("Failed to write trusted workspaces file: {e}")))?;
    Ok(())
}

pub fn is_workspace_trusted(workspace: &Path, config_dir: &Path) -> bool {
    if std::env::var("RHO_TRUST_PROJECT")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
    {
        return true;
    }
    let canonical = canonical_path(workspace);
    let data = load_trusted(config_dir);
    data.trusted.contains(&canonical)
}

pub fn trust_workspace(workspace: &Path, config_dir: &Path) -> Result<()> {
    let canonical = canonical_path(workspace);
    let mut data = load_trusted(config_dir);
    data.trusted.insert(canonical);
    save_trusted(config_dir, &data)
}

pub fn untrust_workspace(workspace: &Path, config_dir: &Path) -> Result<()> {
    let canonical = canonical_path(workspace);
    let mut data = load_trusted(config_dir);
    data.trusted.remove(&canonical);
    save_trusted(config_dir, &data)
}

fn approved_servers_path(config_dir: &Path) -> PathBuf {
    config_dir.join(APPROVED_SERVERS_FILE_NAME)
}

fn load_approved_servers(config_dir: &Path) -> ApprovedServers {
    let path = approved_servers_path(config_dir);
    if !path.exists() {
        return ApprovedServers::default();
    }
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str::<ApprovedServers>(&s).ok())
        .unwrap_or_default()
}

fn save_approved_servers(config_dir: &Path, data: &ApprovedServers) -> Result<()> {
    let path = approved_servers_path(config_dir);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(data)
        .map_err(|e| AppError::Config(format!("Failed to serialize approved servers: {e}")))?;
    std::fs::write(&path, content)
        .map_err(|e| AppError::Config(format!("Failed to write approved servers file: {e}")))?;
    Ok(())
}

fn server_trust_key(workspace: &Path, server_name: &str) -> String {
    let canonical = canonical_path(workspace);
    format!("{}:{}", canonical.to_string_lossy(), server_name)
}

pub fn is_server_definition_trusted(
    workspace: &Path,
    server_name: &str,
    definition_hash: &str,
    config_dir: &Path,
) -> bool {
    if std::env::var("RHO_TRUST_PROJECT")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
    {
        return true;
    }
    let key = server_trust_key(workspace, server_name);
    let approved = load_approved_servers(config_dir);
    approved
        .approved
        .get(&key)
        .map(|h| h == definition_hash)
        .unwrap_or(false)
}

pub fn approve_server_definition(
    workspace: &Path,
    server_name: &str,
    definition_hash: &str,
    config_dir: &Path,
) -> Result<()> {
    let key = server_trust_key(workspace, server_name);
    let mut approved = load_approved_servers(config_dir);
    approved.approved.insert(key, definition_hash.to_string());
    save_approved_servers(config_dir, &approved)
}

pub fn revoke_server_definition(workspace: &Path, server_name: &str, config_dir: &Path) -> Result<()> {
    let key = server_trust_key(workspace, server_name);
    let mut approved = load_approved_servers(config_dir);
    approved.approved.remove(&key);
    save_approved_servers(config_dir, &approved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workspace_trust_lifecycle() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config_dir = temp_dir.path().join("config");
        let workspace = temp_dir.path().join("my_project");
        std::fs::create_dir_all(&workspace).unwrap();

        assert!(!is_workspace_trusted(&workspace, &config_dir));

        trust_workspace(&workspace, &config_dir).unwrap();
        assert!(is_workspace_trusted(&workspace, &config_dir));

        untrust_workspace(&workspace, &config_dir).unwrap();
        assert!(!is_workspace_trusted(&workspace, &config_dir));
    }

    #[test]
    fn test_server_definition_trust_lifecycle() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config_dir = temp_dir.path().join("config");
        let workspace = temp_dir.path().join("my_project");
        std::fs::create_dir_all(&workspace).unwrap();

        let srv = "test_server";
        let hash1 = "hash_v1";
        let hash2 = "hash_v2";

        assert!(!is_server_definition_trusted(&workspace, srv, hash1, &config_dir));

        approve_server_definition(&workspace, srv, hash1, &config_dir).unwrap();
        assert!(is_server_definition_trusted(&workspace, srv, hash1, &config_dir));
        assert!(!is_server_definition_trusted(&workspace, srv, hash2, &config_dir));

        revoke_server_definition(&workspace, srv, &config_dir).unwrap();
        assert!(!is_server_definition_trusted(&workspace, srv, hash1, &config_dir));
    }
}
