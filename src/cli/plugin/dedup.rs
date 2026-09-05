use rho_harness_core::config::PluginConfig;
use std::collections::BTreeMap;
use std::path::PathBuf;

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

pub fn validate_no_duplicates(
    existing_plugins: &BTreeMap<String, PluginConfig>,
    candidate: &PluginCandidate,
) -> Result<(), DuplicatePluginError> {
    let candidate_short = strip_prefix(&candidate.name);

    for (existing_name, existing_cfg) in existing_plugins {
        let existing_short = strip_prefix(existing_name);
        let same_name = existing_name == &candidate.name || existing_short == candidate_short;

        if same_name {
            if !candidate.force {
                return Err(DuplicatePluginError::Name(existing_name.clone()));
            }
            continue;
        }

        if existing_cfg.command.as_deref() == Some(&candidate.command) {
            return Err(DuplicatePluginError::Command {
                existing_plugin: existing_name.clone(),
                command: candidate.command.clone(),
            });
        }

        let candidate_has_path = !candidate.path.as_os_str().is_empty();
        let existing_has_path = !existing_cfg.path.as_os_str().is_empty();

        if candidate_has_path && existing_has_path && existing_cfg.path == candidate.path {
            return Err(DuplicatePluginError::Path {
                existing_plugin: existing_name.clone(),
                path: candidate.path.clone(),
            });
        }

        let existing_file_name = existing_cfg.path.file_name().and_then(|f| f.to_str());
        if existing_has_path && existing_file_name == Some(&candidate.command) {
            return Err(DuplicatePluginError::Command {
                existing_plugin: existing_name.clone(),
                command: candidate.command.clone(),
            });
        }

        let candidate_file_name = candidate.path.file_name().and_then(|f| f.to_str());
        if candidate_has_path && existing_cfg.command.as_deref() == candidate_file_name {
            return Err(DuplicatePluginError::Command {
                existing_plugin: existing_name.clone(),
                command: candidate.command.clone(),
            });
        }
    }

    Ok(())
}

fn strip_prefix(name: &str) -> &str {
    name.strip_prefix("rho-plugin-").unwrap_or(name)
}

#[cfg(test)]
mod tests;
