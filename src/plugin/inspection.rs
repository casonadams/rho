use crate::config::Config;
use crate::error::Result;
use crate::plugin::activation::{PluginValidator, ProtocolPluginValidator};
use crate::plugin::builtin;
use crate::plugin::capability::PluginOrigin;
use crate::plugin::loader::{ConfiguredStatus, DiscoveredCandidate, DiscoveredKind, PluginLoader};
use crate::plugin::resolver::{CapabilityPlugin, CapabilityResolver, PluginResolutionStatus, ResolutionReport};
use std::collections::BTreeMap;
use std::fmt::Write;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfiguredInspection {
    pub name: String,
    pub path: std::path::PathBuf,
    pub package: Option<String>,
    pub status: String,
    pub capabilities: Vec<String>,
    pub replacements: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginInspection {
    pub configured: Vec<ConfiguredInspection>,
    pub ignored: Vec<DiscoveredCandidate>,
    pub resolution: ResolutionReport,
}

pub async fn inspect(config: &Config, project_dir: Option<&Path>) -> Result<PluginInspection> {
    inspect_with(config, project_dir, &ProtocolPluginValidator).await
}

pub async fn inspect_with<V: PluginValidator>(
    config: &Config,
    project_dir: Option<&Path>,
    validator: &V,
) -> Result<PluginInspection> {
    let configured_candidates = PluginLoader::configured_candidates(&config.config_dir, &config.plugins);
    let configured_paths: std::collections::BTreeSet<_> = configured_candidates
        .iter()
        .map(|candidate| candidate.path.clone())
        .collect();
    let discovery = PluginLoader::discover(&config.config_dir, project_dir)?;
    let ignored = discovery
        .candidates
        .into_iter()
        .filter(|candidate| !configured_paths.contains(&candidate.path))
        .collect();

    let mut configured = Vec::new();
    let mut external = Vec::new();
    for candidate in configured_candidates {
        if candidate.status != ConfiguredStatus::Eligible {
            configured.push(ConfiguredInspection {
                name: candidate.name,
                path: candidate.path,
                package: candidate.package,
                status: format!("inactive: {}", candidate.status.label()),
                capabilities: Vec::new(),
                replacements: candidate.replaces.iter().map(ToString::to_string).collect(),
            });
            continue;
        }
        match validator.validate(&candidate.path).await {
            Ok(manifest) if manifest.plugin_id.as_str() == candidate.name => {
                let capabilities = manifest
                    .capabilities
                    .iter()
                    .map(|declaration| declaration.id.to_string())
                    .collect();
                external.push(CapabilityPlugin {
                    manifest,
                    origin: PluginOrigin::Configured {
                        executable: candidate.path.display().to_string(),
                        package: candidate.package.clone(),
                    },
                    authorized_replacements: candidate.replaces.clone(),
                    configured: true,
                });
                configured.push(ConfiguredInspection {
                    name: candidate.name,
                    path: candidate.path,
                    package: candidate.package,
                    status: "validated".to_string(),
                    capabilities,
                    replacements: candidate.replaces.iter().map(ToString::to_string).collect(),
                });
            }
            Ok(_) => configured.push(ConfiguredInspection {
                name: candidate.name,
                path: candidate.path,
                package: candidate.package,
                status: "inactive: configured name does not match plugin identity".to_string(),
                capabilities: Vec::new(),
                replacements: candidate.replaces.iter().map(ToString::to_string).collect(),
            }),
            Err(_) => configured.push(ConfiguredInspection {
                name: candidate.name,
                path: candidate.path,
                package: candidate.package,
                status: "inactive: protocol validation failed".to_string(),
                capabilities: Vec::new(),
                replacements: candidate.replaces.iter().map(ToString::to_string).collect(),
            }),
        }
    }

    let resolution = CapabilityResolver::resolve(vec![builtin::capability_plugin()], external);
    let statuses: BTreeMap<_, _> = resolution
        .plugins
        .iter()
        .map(|plugin| (plugin.plugin_id.as_str(), &plugin.status))
        .collect();
    for plugin in &mut configured {
        if let Some(status) = statuses.get(plugin.name.as_str()) {
            plugin.status = match status {
                PluginResolutionStatus::Active => "active".to_string(),
                PluginResolutionStatus::Rejected { reason } => format!("inactive: {reason}"),
                PluginResolutionStatus::Ignored { reason } => format!("ignored: {reason}"),
            };
        }
    }

    Ok(PluginInspection {
        configured,
        ignored,
        resolution,
    })
}

impl PluginInspection {
    pub fn render_capability(&self, capability: &crate::plugin::capability::CapabilityId) -> String {
        match self.resolution.active.get(capability) {
            Some(active) => {
                let replacement = active
                    .replaces
                    .as_ref()
                    .map(|target| format!("; replaces {target}"))
                    .unwrap_or_default();
                format!(
                    "{capability}: {} from {} ({:?}{replacement})\n",
                    active.id, active.plugin_id, active.origin
                )
            }
            None => format!("{capability}: unavailable\n"),
        }
    }

    pub fn render(&self) -> String {
        let mut output = String::new();
        output.push_str("Configured plugins:\n");
        if self.configured.is_empty() {
            output.push_str("  (none)\n");
        }
        for plugin in &self.configured {
            let origin = plugin.package.as_deref().map_or("local path", |_| "Cargo");
            let _ = writeln!(
                output,
                "  - {} [{}] ({origin}: {})",
                plugin.name,
                plugin.status,
                plugin.path.display()
            );
            for capability in &plugin.capabilities {
                let _ = writeln!(output, "      capability: {capability}");
            }
            for replacement in &plugin.replacements {
                let _ = writeln!(output, "      authorized replacement: {replacement}");
            }
        }

        output.push_str("Active capabilities:\n");
        for (target, active) in &self.resolution.active {
            let replacement = active
                .replaces
                .as_ref()
                .map(|replaces| format!(" replacing {replaces}"))
                .unwrap_or_default();
            let _ = writeln!(
                output,
                "  - {target}: {} from {}{replacement}",
                active.id, active.plugin_id
            );
        }

        output.push_str("Ignored discovery candidates:\n");
        if self.ignored.is_empty() {
            output.push_str("  (none)\n");
        }
        for candidate in &self.ignored {
            let identity = match &candidate.kind {
                DiscoveredKind::Manifest(manifest) => manifest.name.as_str(),
                DiscoveredKind::Executable => candidate
                    .path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("unknown"),
            };
            let _ = writeln!(
                output,
                "  - {identity} [unconfigured; ignored] ({:?}: {})",
                candidate.source,
                candidate.path.display()
            );
        }
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PluginConfig;
    use crate::error::AppError;
    use crate::plugin::capability::{
        CAPABILITY_API_VERSION, CapabilityDeclaration, CapabilityManifest, PLUGIN_PROTOCOL_VERSION, ValidatedManifest,
    };
    use async_trait::async_trait;

    struct Validator(BTreeMap<String, ValidatedManifest>);

    #[async_trait]
    impl PluginValidator for Validator {
        async fn validate(&self, executable: &Path) -> Result<ValidatedManifest> {
            executable
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| self.0.get(name))
                .cloned()
                .ok_or_else(|| AppError::Plugin("invalid".to_string()))
        }
    }

    fn manifest(name: &str, capability: &str, replaces: Option<&str>) -> ValidatedManifest {
        CapabilityManifest {
            plugin_id: name.parse().unwrap(),
            plugin_version: "1.0.0".to_string(),
            api_version: CAPABILITY_API_VERSION,
            protocol_version: PLUGIN_PROTOCOL_VERSION,
            capabilities: vec![CapabilityDeclaration {
                id: capability.parse().unwrap(),
                replaces: replaces.map(|value| value.parse().unwrap()),
            }],
        }
        .validate()
        .unwrap()
    }

    fn executable(path: &Path) {
        std::fs::write(path, b"fixture").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    #[tokio::test]
    async fn inspection_reports_builtins_override_and_ignored_candidate() {
        let root = std::env::temp_dir().join(format!("rho_inspect_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("plugins")).unwrap();
        let configured_path = root.join("container");
        executable(&configured_path);
        executable(&root.join("plugins/rho-plugin-ignored"));
        let mut config = Config {
            config_dir: root.clone(),
            ..Config::default()
        };
        config.plugins.insert(
            "container".to_string(),
            PluginConfig {
                path: configured_path,
                package: None,
                replaces: ["tool:bash".parse().unwrap()].into_iter().collect(),
                ..Default::default()
            },
        );
        let validator = Validator(BTreeMap::from([(
            "container".to_string(),
            manifest("container", "tool:container-bash", Some("tool:bash")),
        )]));
        let inspection = inspect_with(&config, None, &validator).await.unwrap();
        assert_eq!(inspection.configured[0].status, "active");
        assert_eq!(
            inspection.resolution.active[&"tool:bash".parse().unwrap()]
                .plugin_id
                .as_str(),
            "container"
        );
        let rendered = inspection.render();
        assert!(rendered.contains("tool:bash"));
        assert!(rendered.contains("replacing tool:bash"));
        assert!(rendered.contains("rho-plugin-ignored [unconfigured; ignored]"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn invalid_configured_plugin_is_inactive_without_hiding_builtins() {
        let root = std::env::temp_dir().join(format!("rho_inspect_invalid_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("invalid");
        executable(&path);
        let mut config = Config {
            config_dir: root.clone(),
            ..Config::default()
        };
        config.plugins.insert(
            "invalid".to_string(),
            PluginConfig {
                path,
                package: None,
                replaces: Default::default(),
                ..Default::default()
            },
        );
        let inspection = inspect_with(&config, None, &Validator(BTreeMap::new())).await.unwrap();
        assert!(inspection.configured[0].status.contains("protocol validation failed"));
        let bash = "tool:bash".parse().unwrap();
        assert_eq!(inspection.resolution.active[&bash].plugin_id.as_str(), "rho.builtin");
        assert!(inspection.render_capability(&bash).contains("BuiltIn"));
        std::fs::remove_dir_all(root).unwrap();
    }
}
