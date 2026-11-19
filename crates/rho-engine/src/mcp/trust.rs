use rho_harness_core::config::{Config, McpServerConfig};
use rho_harness_core::error::{AppError, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const APPROVED_SERVERS_FILE_NAME: &str = "mcp_approved_servers.json";

#[derive(Debug, Default, Deserialize, Serialize)]
struct ApprovedServers {
    #[serde(default)]
    approved: BTreeMap<String, String>,
}

fn canonical_path(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
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

fn server_trust_key(workspace: &Path, server_name: &str) -> String {
    let canonical = canonical_path(workspace);
    format!("{}:{}", canonical.to_string_lossy(), server_name)
}

pub fn project_trusted_by_env() -> bool {
    std::env::var("RHO_TRUST_PROJECT").is_ok_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
}

pub fn is_server_definition_trusted(
    workspace: &Path,
    server_name: &str,
    definition_hash: &str,
    config_dir: &Path,
) -> bool {
    if project_trusted_by_env() {
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

pub fn project_server_approval(config: &Config, name: &str, server: &McpServerConfig) -> Option<bool> {
    let project = config.mcp.project.as_ref().filter(|p| p.names.contains(name))?;
    let hash = super::cache::compute_definition_hash(server);
    Some(is_server_definition_trusted(
        &project.workspace,
        name,
        &hash,
        &config.config_dir,
    ))
}

pub fn approve_server_definition(
    workspace: &Path,
    server_name: &str,
    definition_hash: &str,
    config_dir: &Path,
) -> Result<()> {
    let path = approved_servers_path(config_dir);
    let mut approvals = match std::fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str::<ApprovedServers>(&content)
            .map_err(|e| AppError::Config(format!("Failed to parse {}: {e}", path.display())))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => ApprovedServers::default(),
        Err(e) => return Err(AppError::Config(format!("Failed to read {}: {e}", path.display()))),
    };
    approvals
        .approved
        .insert(server_trust_key(workspace, server_name), definition_hash.to_string());
    let mut bytes = serde_json::to_vec_pretty(&approvals).map_err(|e| AppError::Config(e.to_string()))?;
    bytes.push(b'\n');
    rho_harness_core::fs::write_atomic(&path, &bytes, 0o600)
        .map_err(|e| AppError::Config(format!("Failed to write {}: {e}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approved_server_definition_is_trusted_only_for_matching_hash() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config_dir = temp_dir.path().join("config");
        let workspace = temp_dir.path().join("my_project");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::create_dir_all(&config_dir).unwrap();

        let srv = "test_server";
        assert!(!is_server_definition_trusted(&workspace, srv, "hash_v1", &config_dir));

        let key = server_trust_key(&workspace, srv);
        let approvals = serde_json::json!({ "approved": { (key): "hash_v1" } });
        std::fs::write(approved_servers_path(&config_dir), approvals.to_string()).unwrap();
        assert!(is_server_definition_trusted(&workspace, srv, "hash_v1", &config_dir));
        assert!(!is_server_definition_trusted(&workspace, srv, "hash_v2", &config_dir));
    }

    #[test]
    fn approving_keeps_other_approvals_and_replaces_the_previous_hash() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config_dir = temp_dir.path().join("config");
        let workspace = temp_dir.path().join("my_project");
        std::fs::create_dir_all(&workspace).unwrap();

        approve_server_definition(&workspace, "a", "a_v1", &config_dir).unwrap();
        approve_server_definition(&workspace, "b", "b_v1", &config_dir).unwrap();
        approve_server_definition(&workspace, "a", "a_v2", &config_dir).unwrap();

        assert!(is_server_definition_trusted(&workspace, "a", "a_v2", &config_dir));
        assert!(!is_server_definition_trusted(&workspace, "a", "a_v1", &config_dir));
        assert!(is_server_definition_trusted(&workspace, "b", "b_v1", &config_dir));
    }

    #[test]
    fn approving_refuses_to_overwrite_an_unreadable_approvals_file() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = approved_servers_path(temp_dir.path());
        std::fs::write(&path, "not json").unwrap();

        let err = approve_server_definition(temp_dir.path(), "a", "h", temp_dir.path()).unwrap_err();

        assert!(err.to_string().contains("Failed to parse"), "{err}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "not json");
    }
}
