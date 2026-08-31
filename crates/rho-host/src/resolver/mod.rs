use rho_sdk::capability::{ActiveCapability, CapabilityId, CapabilityKind, PluginOrigin, ValidatedManifest};
use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityPlugin {
    pub manifest: ValidatedManifest,
    pub origin: PluginOrigin,
    pub authorized_replacements: BTreeSet<CapabilityId>,
    pub configured: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginResolutionStatus {
    Active,
    Rejected { reason: String },
    Ignored { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginResolution {
    pub plugin_id: rho_sdk::capability::PluginId,
    pub origin: PluginOrigin,
    pub capabilities: Vec<CapabilityId>,
    pub status: PluginResolutionStatus,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResolutionReport {
    pub active: BTreeMap<CapabilityId, ActiveCapability>,
    pub plugins: Vec<PluginResolution>,
}

pub struct CapabilityResolver;

impl CapabilityResolver {
    pub fn resolve(mut built_ins: Vec<CapabilityPlugin>, mut external: Vec<CapabilityPlugin>) -> ResolutionReport {
        built_ins.sort_by_key(plugin_order_key);
        external.sort_by_key(plugin_order_key);
        let mut report = ResolutionReport::default();
        let mut seen_plugin_ids = BTreeSet::new();
        for plugin in built_ins {
            if seen_plugin_ids.insert(plugin.manifest.plugin_id.clone()) {
                resolve_plugin(&mut report, plugin, true);
            } else {
                reject_plugin(&mut report, plugin, "duplicate stable plugin identity");
            }
        }
        for plugin in external {
            if !seen_plugin_ids.insert(plugin.manifest.plugin_id.clone()) {
                reject_plugin(&mut report, plugin, "duplicate stable plugin identity");
            } else if plugin.configured && !matches!(plugin.origin, PluginOrigin::Configured { .. }) {
                reject_plugin(
                    &mut report,
                    plugin,
                    "configured plugin does not have a configured origin",
                );
            } else if plugin.configured {
                resolve_plugin(&mut report, plugin, false);
            } else {
                report.plugins.push(PluginResolution {
                    plugin_id: plugin.manifest.plugin_id,
                    origin: plugin.origin,
                    capabilities: plugin
                        .manifest
                        .capabilities
                        .into_iter()
                        .map(|declaration| declaration.id)
                        .collect(),
                    status: PluginResolutionStatus::Ignored {
                        reason: "not declared in global config.toml".to_string(),
                    },
                });
            }
        }
        report
    }
}

fn plugin_order_key(plugin: &CapabilityPlugin) -> (String, String, String) {
    let origin = match &plugin.origin {
        PluginOrigin::BuiltIn => "builtin".to_string(),
        PluginOrigin::Configured { executable, package } => {
            format!("configured:{executable}:{}", package.as_deref().unwrap_or_default())
        }
    };
    let manifest = serde_json::to_string(&plugin.manifest).unwrap_or_default();
    (plugin.manifest.plugin_id.to_string(), origin, manifest)
}

fn reject_plugin(report: &mut ResolutionReport, plugin: CapabilityPlugin, reason: &str) {
    report.plugins.push(PluginResolution {
        plugin_id: plugin.manifest.plugin_id,
        origin: plugin.origin,
        capabilities: plugin
            .manifest
            .capabilities
            .into_iter()
            .map(|declaration| declaration.id)
            .collect(),
        status: PluginResolutionStatus::Rejected {
            reason: reason.to_string(),
        },
    });
}

fn resolve_plugin(report: &mut ResolutionReport, plugin: CapabilityPlugin, built_in: bool) {
    let capability_ids: Vec<_> = plugin
        .manifest
        .capabilities
        .iter()
        .map(|declaration| declaration.id.clone())
        .collect();
    let mut target_keys = BTreeSet::new();
    let validation = plugin.manifest.capabilities.iter().try_for_each(|declaration| {
        let key = if let Some(target) = &declaration.replaces {
            if built_in {
                return Err("built-in capabilities cannot declare replacements".to_string());
            }
            if !plugin.authorized_replacements.contains(target) {
                return Err(format!("replacement of {target} is not authorized by config.toml"));
            }
            if !report.active.contains_key(target) {
                return Err(format!("replacement target {target} is not active"));
            }
            target
        } else {
            if report.active.contains_key(&declaration.id) {
                return Err(format!(
                    "capability {} conflicts without an authorized replacement",
                    declaration.id
                ));
            }
            // A session resolves exactly one presentation capability: a second
            // ui capability must go through the authorized replacement path.
            if declaration.id.kind() == CapabilityKind::Ui
                && report.active.keys().any(|active| active.kind() == CapabilityKind::Ui)
            {
                return Err("a second presentation capability conflicts with the active one".to_string());
            }
            &declaration.id
        };
        if !target_keys.insert(key.clone()) {
            return Err(format!("plugin declares target {key} more than once"));
        }
        Ok(())
    });

    if let Err(reason) = validation {
        report.plugins.push(PluginResolution {
            plugin_id: plugin.manifest.plugin_id,
            origin: plugin.origin,
            capabilities: capability_ids,
            status: PluginResolutionStatus::Rejected { reason },
        });
        return;
    }

    for declaration in plugin.manifest.capabilities {
        let key = declaration.replaces.clone().unwrap_or_else(|| declaration.id.clone());
        report.active.insert(
            key,
            ActiveCapability {
                id: declaration.id,
                plugin_id: plugin.manifest.plugin_id.clone(),
                origin: plugin.origin.clone(),
                replaces: declaration.replaces,
            },
        );
    }
    report.plugins.push(PluginResolution {
        plugin_id: plugin.manifest.plugin_id,
        origin: plugin.origin,
        capabilities: capability_ids,
        status: PluginResolutionStatus::Active,
    });
}
