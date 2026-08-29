use crate::config::PluginConfig;
use crate::error::{AppError, Result};
use crate::plugin::types::PluginManifest;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, btree_map::Entry};
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoverySource {
    GlobalDirectory,
    WorkspaceDirectory,
    CargoBin,
    Path,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscoveredKind {
    Manifest(PluginManifest),
    Executable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredCandidate {
    pub path: PathBuf,
    pub source: DiscoverySource,
    pub kind: DiscoveredKind,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PluginDiscovery {
    pub candidates: Vec<DiscoveredCandidate>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfiguredStatus {
    Eligible,
    Missing,
    NotAFile,
    NotExecutable,
}

impl ConfiguredStatus {
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Eligible => "eligible",
            Self::Missing => "missing",
            Self::NotAFile => "not a file",
            Self::NotExecutable => "not executable",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfiguredCandidate {
    pub name: String,
    pub path: PathBuf,
    pub package: Option<String>,
    pub replaces: std::collections::BTreeSet<crate::plugin::capability::CapabilityId>,
    pub status: ConfiguredStatus,
}

pub struct PluginLoader;

struct DiscoveryPaths<'a> {
    cargo_bin: Option<&'a Path>,
    path_dirs: &'a [PathBuf],
}

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

    fn discover_from(
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
        configured
            .iter()
            .map(|(name, plugin)| {
                let joined = if plugin.path.is_absolute() {
                    plugin.path.clone()
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
            if path.is_dir() {
                let manifest_path = path.join("plugin.toml");
                let Ok(content) = std::fs::read_to_string(&manifest_path) else {
                    continue;
                };
                let Ok(manifest) = toml::from_str::<PluginManifest>(&content) else {
                    continue;
                };
                if manifest.api_version == 1 {
                    insert_candidate(
                        candidates,
                        DiscoveredCandidate {
                            path: canonical_or_normalized(&path),
                            source,
                            kind: DiscoveredKind::Manifest(manifest),
                        },
                    );
                }
            } else if path.is_file() && is_plugin_binary(&path) {
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
    match &candidate.kind {
        DiscoveredKind::Manifest(manifest) => manifest.name.clone(),
        DiscoveredKind::Executable => candidate
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_string(),
    }
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
mod tests {
    use super::*;

    fn executable(path: &Path) {
        std::fs::write(path, b"fixture").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    #[test]
    fn discovery_is_sorted_deduplicated_and_informational() {
        let root = std::env::temp_dir().join(format!("rho_discovery_{}", uuid::Uuid::new_v4()));
        let global = root.join("config/plugins");
        let workspace = root.join("project/.rho/plugins");
        let path_dir = root.join("path");
        std::fs::create_dir_all(&global).unwrap();
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::create_dir_all(&path_dir).unwrap();
        executable(&global.join("rho-plugin-zed"));
        executable(&path_dir.join("rho-plugin-alpha"));
        #[cfg(unix)]
        std::os::unix::fs::symlink(path_dir.join("rho-plugin-alpha"), workspace.join("rho-plugin-alpha")).unwrap();

        let discovery = PluginLoader::discover_from(
            &root.join("config"),
            Some(&root.join("project")),
            DiscoveryPaths {
                cargo_bin: None,
                path_dirs: std::slice::from_ref(&path_dir),
            },
        )
        .unwrap();
        let names: Vec<_> = discovery
            .candidates
            .iter()
            .map(|candidate| candidate.path.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert_eq!(names, ["rho-plugin-alpha", "rho-plugin-zed"]);
        #[cfg(unix)]
        assert_eq!(discovery.candidates[0].source, DiscoverySource::WorkspaceDirectory);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn only_configuration_creates_eligible_candidates() {
        let root = std::env::temp_dir().join(format!("rho_activation_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("bin")).unwrap();
        executable(&root.join("bin/rho-plugin-configured"));
        executable(&root.join("bin/rho-plugin-absolute"));
        executable(&root.join("bin/rho-plugin-ignored"));
        let configured = BTreeMap::from([
            (
                "absolute".to_string(),
                PluginConfig {
                    path: root.join("bin/rho-plugin-absolute"),
                    package: None,
                    replaces: Default::default(),
                },
            ),
            (
                "configured".to_string(),
                PluginConfig {
                    path: PathBuf::from("bin/../bin/rho-plugin-configured"),
                    package: None,
                    replaces: Default::default(),
                },
            ),
        ]);
        let candidates = PluginLoader::configured_candidates(&root, &configured);
        assert_eq!(candidates.len(), 2);
        assert!(
            candidates
                .iter()
                .all(|candidate| candidate.status == ConfiguredStatus::Eligible)
        );
        assert!(candidates.iter().all(|candidate| candidate.path.is_absolute()));
        assert!(
            candidates
                .iter()
                .all(|candidate| !candidate.path.to_string_lossy().contains("ignored"))
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn configured_candidate_reports_missing_and_non_executable_paths() {
        let root = std::env::temp_dir().join(format!("rho_activation_invalid_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("plain"), b"fixture").unwrap();
        let configured = BTreeMap::from([
            (
                "missing".to_string(),
                PluginConfig {
                    path: "missing".into(),
                    package: None,
                    replaces: Default::default(),
                },
            ),
            (
                "plain".to_string(),
                PluginConfig {
                    path: "plain".into(),
                    package: None,
                    replaces: Default::default(),
                },
            ),
        ]);
        let candidates = PluginLoader::configured_candidates(&root, &configured);
        assert_eq!(candidates[0].status, ConfiguredStatus::Missing);
        #[cfg(unix)]
        assert_eq!(candidates[1].status, ConfiguredStatus::NotExecutable);
        std::fs::remove_dir_all(root).unwrap();
    }
}
