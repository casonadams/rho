pub mod types;

use rho_core::config::PluginConfig;
use rho_core::error::{AppError, Result};
use std::collections::{BTreeMap, btree_map::Entry};
use std::path::{Component, Path, PathBuf};

use types::DiscoveryPaths;
pub use types::{
    ConfiguredCandidate, ConfiguredStatus, DiscoveredCandidate, DiscoveredKind, DiscoverySource, PluginDiscovery,
};

pub struct PluginLoader;

impl PluginLoader {
    pub fn discover(config_dir: &Path, project_dir: Option<&Path>) -> Result<PluginDiscovery> {
        let cargo_bin = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .ok()
            .map(|home| PathBuf::from(home).join(".cargo").join("bin"));
        let path_dirs: Vec<PathBuf> = std::env::var_os("PATH")
            .map(|value| std::env::split_paths(&value).collect())
            .unwrap_or_default();
        Self::discover_from(
            config_dir,
            project_dir,
            DiscoveryPaths {
                cargo_bin: cargo_bin.as_deref(),
                path_dirs: &path_dirs,
            },
        )
    }

    pub(crate) fn discover_from(
        config_dir: &Path,
        project_dir: Option<&Path>,
        discovery_paths: DiscoveryPaths<'_>,
    ) -> Result<PluginDiscovery> {
        let mut candidates = BTreeMap::new();
        Self::scan_plugin_directory(
            &config_dir.join("plugins"),
            DiscoverySource::GlobalDirectory,
            &mut candidates,
        )?;
        if let Some(project_dir) = project_dir {
            Self::scan_plugin_directory(
                &project_dir.join(".rho").join("plugins"),
                DiscoverySource::WorkspaceDirectory,
                &mut candidates,
            )?;
        }
        if let Some(cargo_bin) = discovery_paths.cargo_bin {
            Self::scan_executables(cargo_bin, DiscoverySource::CargoBin, &mut candidates)?;
        }
        let mut path_dirs = discovery_paths.path_dirs.to_vec();
        path_dirs.sort();
        path_dirs.dedup();
        for path_dir in path_dirs {
            Self::scan_executables(&path_dir, DiscoverySource::Path, &mut candidates)?;
        }

        let mut candidates: Vec<_> = candidates.into_values().collect();
        candidates.sort_by(|left, right| {
            candidate_identity(left)
                .cmp(&candidate_identity(right))
                .then_with(|| left.path.cmp(&right.path))
                .then_with(|| left.source.cmp(&right.source))
        });
        Ok(PluginDiscovery { candidates })
    }

    pub fn configured_candidates(
        config_dir: &Path,
        configured: &BTreeMap<String, PluginConfig>,
    ) -> Vec<ConfiguredCandidate> {
        let project_dir = std::env::current_dir().ok();
        Self::configured_candidates_with_project(config_dir, project_dir.as_deref(), configured)
    }

    pub fn configured_candidates_with_project(
        config_dir: &Path,
        project_dir: Option<&Path>,
        configured: &BTreeMap<String, PluginConfig>,
    ) -> Vec<ConfiguredCandidate> {
        configured
            .iter()
            .map(|(name, plugin)| {
                let joined = if plugin.path.is_absolute() {
                    plugin.path.clone()
                } else if config_dir.join(&plugin.path).exists() {
                    config_dir.join(&plugin.path)
                } else if let Some(proj) = project_dir
                    && proj.join(".rho").join(&plugin.path).exists()
                {
                    proj.join(".rho").join(&plugin.path)
                } else if let Some(proj) = project_dir
                    && proj.join(&plugin.path).exists()
                {
                    proj.join(&plugin.path)
                } else {
                    config_dir.join(&plugin.path)
                };
                let normalized = normalize_path(&joined);
                let (path, status) = match std::fs::canonicalize(&normalized) {
                    Ok(path) if !path.is_file() => (path, ConfiguredStatus::NotAFile),
                    Ok(path) if !is_executable(&path) => (path, ConfiguredStatus::NotExecutable),
                    Ok(path) => (path, ConfiguredStatus::Eligible),
                    Err(_) if normalized.exists() => (normalized, ConfiguredStatus::NotAFile),
                    Err(_) => (normalized, ConfiguredStatus::Missing),
                };
                ConfiguredCandidate {
                    name: name.clone(),
                    path,
                    package: plugin.package.clone(),
                    replaces: plugin.replaces.clone(),
                    status,
                }
            })
            .collect()
    }

    fn scan_plugin_directory(
        directory: &Path,
        source: DiscoverySource,
        candidates: &mut BTreeMap<PathBuf, DiscoveredCandidate>,
    ) -> Result<()> {
        for path in sorted_entries(directory)? {
            if path.is_file() && is_plugin_binary(&path) {
                insert_candidate(
                    candidates,
                    DiscoveredCandidate {
                        path: canonical_or_normalized(&path),
                        source,
                        kind: DiscoveredKind::Executable,
                    },
                );
            }
        }
        Ok(())
    }

    fn scan_executables(
        directory: &Path,
        source: DiscoverySource,
        candidates: &mut BTreeMap<PathBuf, DiscoveredCandidate>,
    ) -> Result<()> {
        for path in sorted_entries(directory)? {
            if path.is_file() && is_plugin_binary(&path) {
                insert_candidate(
                    candidates,
                    DiscoveredCandidate {
                        path: canonical_or_normalized(&path),
                        source,
                        kind: DiscoveredKind::Executable,
                    },
                );
            }
        }
        Ok(())
    }
}

fn sorted_entries(directory: &Path) -> Result<Vec<PathBuf>> {
    if !directory.is_dir() {
        return Ok(Vec::new());
    }
    let mut entries: Vec<_> = std::fs::read_dir(directory)
        .map_err(|error| AppError::Config(format!("Failed to scan plugin directory: {error}")))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .collect();
    entries.sort();
    Ok(entries)
}

fn insert_candidate(candidates: &mut BTreeMap<PathBuf, DiscoveredCandidate>, candidate: DiscoveredCandidate) {
    match candidates.entry(candidate.path.clone()) {
        Entry::Vacant(entry) => {
            entry.insert(candidate);
        }
        Entry::Occupied(mut entry) if candidate.source < entry.get().source => {
            entry.insert(candidate);
        }
        Entry::Occupied(_) => {}
    }
}

fn candidate_identity(candidate: &DiscoveredCandidate) -> String {
    candidate
        .path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_string()
}

fn is_plugin_binary(path: &Path) -> bool {
    let file_name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default();
    file_name.starts_with("rho-plugin-")
        || file_name.starts_with("rho_plugin_")
        || (file_name.starts_with("rho-") && file_name != "rho" && file_name != "rho-plugin")
}

fn canonical_or_normalized(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| normalize_path(path))
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir if normalized.file_name().is_some() => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

#[cfg(test)]
mod tests;
