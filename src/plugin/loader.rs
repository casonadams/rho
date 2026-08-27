use crate::error::{AppError, Result};
use crate::plugin::registry::ExtensionRegistry;
use crate::plugin::types::PluginManifest;
use std::path::{Path, PathBuf};

pub struct PluginDiscovery {
    pub manifests: Vec<(PathBuf, PluginManifest)>,
    pub binary_plugins: Vec<PathBuf>,
}

pub struct PluginLoader;

impl PluginLoader {
    pub fn discover(config_dir: &Path, project_dir: Option<&Path>) -> Result<PluginDiscovery> {
        let mut manifests = Vec::new();
        let mut binary_plugins = Vec::new();

        let global_plugins = config_dir.join("plugins");
        Self::scan_directory(&global_plugins, &mut manifests, &mut binary_plugins)?;

        if let Some(proj) = project_dir {
            let local_plugins = proj.join(".rho").join("plugins");
            Self::scan_directory(&local_plugins, &mut manifests, &mut binary_plugins)?;
        }

        if let Ok(home) = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) {
            let cargo_bin = PathBuf::from(home).join(".cargo").join("bin");
            if cargo_bin.exists() && cargo_bin.is_dir() {
                Self::scan_binaries(&cargo_bin, &mut binary_plugins);
            }
        }

        if let Ok(path_var) = std::env::var("PATH") {
            for entry in std::env::split_paths(&path_var) {
                if entry.exists() && entry.is_dir() {
                    Self::scan_binaries(&entry, &mut binary_plugins);
                }
            }
        }

        manifests.sort_by(|left, right| left.1.name.cmp(&right.1.name).then_with(|| left.0.cmp(&right.0)));
        binary_plugins.sort();

        Ok(PluginDiscovery {
            manifests,
            binary_plugins,
        })
    }

    fn scan_directory(
        dir: &Path,
        manifests: &mut Vec<(PathBuf, PluginManifest)>,
        binary_plugins: &mut Vec<PathBuf>,
    ) -> Result<()> {
        if !dir.exists() || !dir.is_dir() {
            return Ok(());
        }

        let entries = std::fs::read_dir(dir).map_err(|e| AppError::Config(e.to_string()))?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let manifest_path = path.join("plugin.toml");
                if manifest_path.exists()
                    && let Ok(content) = std::fs::read_to_string(&manifest_path)
                    && let Ok(manifest) = toml::from_str::<PluginManifest>(&content)
                    && manifest.api_version == 1
                {
                    manifests.push((path, manifest));
                }
            } else if path.is_file() {
                Self::check_binary_plugin(&path, binary_plugins);
            }
        }

        Ok(())
    }

    fn scan_binaries(dir: &Path, binary_plugins: &mut Vec<PathBuf>) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    Self::check_binary_plugin(&path, binary_plugins);
                }
            }
        }
    }

    fn check_binary_plugin(path: &Path, binary_plugins: &mut Vec<PathBuf>) {
        let file_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        let is_plugin = file_name.starts_with("rho-plugin-")
            || file_name.starts_with("rho_plugin_")
            || (file_name.starts_with("rho-") && file_name != "rho" && file_name != "rho-plugin");
        if is_plugin && !binary_plugins.contains(&path.to_path_buf()) {
            binary_plugins.push(path.to_path_buf());
        }
    }

    pub fn load_discovered(_discovery: &PluginDiscovery, registry: &mut ExtensionRegistry) -> Result<()> {
        let _ = registry;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_discovery_empty() {
        let temp = std::env::temp_dir().join(format!("rho_plugin_test_{}", uuid::Uuid::new_v4()));
        let discovery = PluginLoader::discover(&temp, None).unwrap();
        assert!(discovery.manifests.is_empty());
    }

    #[test]
    fn test_plugin_discovery_manifest() {
        let temp = std::env::temp_dir().join(format!("rho_plugin_test_{}", uuid::Uuid::new_v4()));
        let plugins_dir = temp.join("plugins").join("my_plugin");
        std::fs::create_dir_all(&plugins_dir).unwrap();
        std::fs::write(
            plugins_dir.join("plugin.toml"),
            r#"
name = "test-plugin"
version = "0.1.0"
description = "A test plugin"
"#,
        )
        .unwrap();

        let discovery = PluginLoader::discover(&temp, None).unwrap();
        assert_eq!(discovery.manifests.len(), 1);
        assert_eq!(discovery.manifests[0].1.name, "test-plugin");
        assert_eq!(discovery.manifests[0].1.version, "0.1.0");
    }
}
