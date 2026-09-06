use rho_harness_core::config::PluginConfig;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DuplicatePluginError {
    #[error("plugin '{0}' is already configured (use --force or --replace to overwrite)")]
    Name(String),

    #[error("plugin command '{command}' collides with existing plugin '{existing_plugin}'")]
    Command { existing_plugin: String, command: String },

    #[error("plugin executable path '{path}' collides with existing plugin '{existing_plugin}'")]
    Path { existing_plugin: String, path: PathBuf },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginCandidate {
    pub name: String,
    pub command: String,
    pub path: PathBuf,
    pub force: bool,
}

fn cmd_err(existing_plugin: &str, command: &str) -> DuplicatePluginError {
    DuplicatePluginError::Command {
        existing_plugin: existing_plugin.to_string(),
        command: command.to_string(),
    }
}

fn check_name_conflict(existing_name: &str, candidate: &PluginCandidate) -> Result<bool, DuplicatePluginError> {
    let same = existing_name == candidate.name || strip_prefix(existing_name) == strip_prefix(&candidate.name);
    if same {
        if !candidate.force {
            return Err(DuplicatePluginError::Name(existing_name.to_string()));
        }
        return Ok(true);
    }
    Ok(false)
}

fn check_path_conflict(
    existing_name: &str,
    existing_path: &Path,
    candidate_path: &Path,
) -> Result<(), DuplicatePluginError> {
    if !candidate_path.as_os_str().is_empty()
        && !existing_path.as_os_str().is_empty()
        && existing_path == candidate_path
    {
        return Err(DuplicatePluginError::Path {
            existing_plugin: existing_name.to_string(),
            path: candidate_path.to_path_buf(),
        });
    }
    Ok(())
}

fn check_command_overlap(
    existing_name: &str,
    existing_cfg: &PluginConfig,
    candidate: &PluginCandidate,
) -> Result<(), DuplicatePluginError> {
    if existing_cfg.command.as_deref() == Some(&candidate.command) {
        return Err(cmd_err(existing_name, &candidate.command));
    }
    let cand_p = (!candidate.path.as_os_str().is_empty()).then_some(candidate.path.as_path());
    let exist_p = (!existing_cfg.path.as_os_str().is_empty()).then_some(existing_cfg.path.as_path());
    let cand_matches = cand_p.is_some_and(|p| {
        existing_cfg.command.as_deref() == p.to_str()
            || existing_cfg.command.as_deref() == p.file_name().and_then(|f| f.to_str())
    });
    let exist_matches = exist_p.is_some_and(|p| {
        p.to_str() == Some(&candidate.command) || p.file_name().and_then(|f| f.to_str()) == Some(&candidate.command)
    });
    if cand_matches || exist_matches {
        return Err(cmd_err(existing_name, &candidate.command));
    }
    Ok(())
}

pub fn validate_no_duplicates(
    existing_plugins: &BTreeMap<String, PluginConfig>,
    candidate: &PluginCandidate,
) -> Result<(), DuplicatePluginError> {
    for (existing_name, existing_cfg) in existing_plugins {
        if check_name_conflict(existing_name, candidate)? {
            continue;
        }
        check_path_conflict(existing_name, &existing_cfg.path, &candidate.path)?;
        check_command_overlap(existing_name, existing_cfg, candidate)?;
    }
    Ok(())
}

fn strip_prefix(name: &str) -> &str {
    name.strip_prefix("rho-plugin-").unwrap_or(name)
}

#[cfg(test)]
mod tests;
