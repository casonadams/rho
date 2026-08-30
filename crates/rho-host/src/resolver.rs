use rho_sdk::capability::{ActiveCapability, CapabilityId, CapabilityKind, PluginOrigin, ValidatedManifest};
use std::collections::{BTreeMap, BTreeSet};

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

#[cfg(test)]
mod tests {
    use super::*;
    use rho_sdk::capability::{
        CAPABILITY_API_VERSION, CapabilityDeclaration, CapabilityManifest, PLUGIN_PROTOCOL_VERSION,
    };

    fn id(value: &str) -> CapabilityId {
        value.parse().unwrap()
    }

    fn plugin(name: &str, declarations: &[(&str, Option<&str>)], settings: (bool, &[&str])) -> CapabilityPlugin {
        let (configured, authorized) = settings;
        let manifest = CapabilityManifest {
            plugin_id: name.parse().unwrap(),
            plugin_version: "1.0.0".to_string(),
            api_version: CAPABILITY_API_VERSION,
            protocol_version: PLUGIN_PROTOCOL_VERSION,
            capabilities: declarations
                .iter()
                .map(|(capability, replaces)| CapabilityDeclaration {
                    id: id(capability),
                    replaces: replaces.map(id),
                })
                .collect(),
        }
        .validate()
        .unwrap();
        CapabilityPlugin {
            manifest,
            origin: if configured {
                PluginOrigin::Configured {
                    executable: format!("/plugins/{name}"),
                    package: None,
                }
            } else {
                PluginOrigin::BuiltIn
            },
            authorized_replacements: authorized.iter().map(|value| id(value)).collect(),
            configured,
        }
    }

    fn built_in() -> CapabilityPlugin {
        plugin(
            "rho.builtin",
            &[("tool:bash", None), ("tool:read", None), ("provider:openai", None)],
            (false, &[]),
        )
    }

    #[test]
    fn configured_plugins_add_and_explicitly_replace_capabilities() {
        let report = CapabilityResolver::resolve(
            vec![built_in()],
            vec![
                plugin("additions", &[("tool:review", None)], (true, &[])),
                plugin(
                    "container",
                    &[("tool:container-bash", Some("tool:bash"))],
                    (true, &["tool:bash"]),
                ),
            ],
        );
        assert_eq!(report.active[&id("tool:review")].plugin_id.as_str(), "additions");
        assert_eq!(report.active[&id("tool:bash")].plugin_id.as_str(), "container");
        assert_eq!(report.active[&id("tool:bash")].replaces, Some(id("tool:bash")));
    }

    #[test]
    fn undeclared_and_unauthorized_plugins_never_change_the_active_set() {
        let report = CapabilityResolver::resolve(
            vec![built_in()],
            vec![
                plugin("ignored", &[("tool:ignored", None)], (false, &[])),
                plugin("unauthorized", &[("tool:replacement", Some("tool:bash"))], (true, &[])),
                plugin("conflict", &[("tool:read", None)], (true, &[])),
            ],
        );
        assert!(!report.active.contains_key(&id("tool:ignored")));
        assert_eq!(report.active[&id("tool:bash")].plugin_id.as_str(), "rho.builtin");
        assert_eq!(report.active[&id("tool:read")].plugin_id.as_str(), "rho.builtin");
        assert!(
            report
                .plugins
                .iter()
                .any(|plugin| matches!(plugin.status, PluginResolutionStatus::Ignored { .. }))
        );
        assert!(
            report
                .plugins
                .iter()
                .filter(|plugin| matches!(plugin.status, PluginResolutionStatus::Rejected { .. }))
                .count()
                >= 2
        );
    }

    #[test]
    fn rejection_is_atomic_and_preserves_the_previous_capabilities() {
        let report = CapabilityResolver::resolve(
            vec![built_in()],
            vec![plugin(
                "partial",
                &[("tool:new", None), ("tool:read", None)],
                (true, &[]),
            )],
        );
        assert!(!report.active.contains_key(&id("tool:new")));
        assert_eq!(report.active[&id("tool:read")].plugin_id.as_str(), "rho.builtin");
    }

    #[test]
    fn resolution_is_identical_for_every_input_order() {
        let left = CapabilityResolver::resolve(
            vec![built_in()],
            vec![
                plugin("zeta", &[("tool:zeta", None)], (true, &[])),
                plugin("alpha", &[("tool:alpha", None)], (true, &[])),
            ],
        );
        let right = CapabilityResolver::resolve(
            vec![built_in()],
            vec![
                plugin("alpha", &[("tool:alpha", None)], (true, &[])),
                plugin("zeta", &[("tool:zeta", None)], (true, &[])),
            ],
        );
        assert_eq!(left, right);
    }

    #[test]
    fn two_replacements_follow_stable_plugin_identity_order() {
        let report = CapabilityResolver::resolve(
            vec![built_in()],
            vec![
                plugin("zeta", &[("tool:zeta-bash", Some("tool:bash"))], (true, &["tool:bash"])),
                plugin(
                    "alpha",
                    &[("tool:alpha-bash", Some("tool:bash"))],
                    (true, &["tool:bash"]),
                ),
            ],
        );
        assert_eq!(report.active[&id("tool:bash")].plugin_id.as_str(), "zeta");
    }

    #[test]
    fn two_ui_presenters_reject_without_authorized_replacement_and_the_bundled_stays_active() {
        let report = CapabilityResolver::resolve(
            vec![plugin(
                "rho.builtin",
                &[("tool:bash", None), ("ui:bundled-terminal", None)],
                (false, &[]),
            )],
            vec![plugin("second-ui", &[("ui:second", None)], (true, &[]))],
        );
        let second = report
            .plugins
            .iter()
            .find(|resolution| resolution.plugin_id.as_str() == "second-ui")
            .unwrap();
        assert!(matches!(second.status, PluginResolutionStatus::Rejected { .. }));
        assert_eq!(
            report.active[&id("ui:bundled-terminal")].plugin_id.as_str(),
            "rho.builtin"
        );
        assert_eq!(report.active.len(), 2);
    }

    #[test]
    fn an_authorized_ui_replacement_deactivates_the_bundled_presenter() {
        let report = CapabilityResolver::resolve(
            vec![plugin(
                "rho.builtin",
                &[("tool:bash", None), ("ui:bundled", None)],
                (false, &[]),
            )],
            vec![plugin(
                "fancy-ui",
                &[("ui:fancy", Some("ui:bundled"))],
                (true, &["ui:bundled"]),
            )],
        );
        assert_eq!(report.active[&id("ui:bundled")].plugin_id.as_str(), "fancy-ui");
        assert_eq!(report.active[&id("ui:bundled")].replaces, Some(id("ui:bundled")));
    }

    #[test]
    fn context_and_command_capabilities_resolve_cleanly() {
        let report = CapabilityResolver::resolve(
            vec![built_in()],
            vec![plugin(
                "kiln",
                &[
                    ("context:kiln", None),
                    ("command:kiln", None),
                    ("tool:kiln_search", None),
                ],
                (true, &[]),
            )],
        );
        assert_eq!(report.active[&id("context:kiln")].plugin_id.as_str(), "kiln");
        assert_eq!(report.active[&id("command:kiln")].plugin_id.as_str(), "kiln");
        assert_eq!(report.active[&id("tool:kiln_search")].plugin_id.as_str(), "kiln");
    }

    #[test]
    fn a_failed_ui_plugin_leaves_unrelated_capabilities_active() {
        let report = CapabilityResolver::resolve(
            vec![built_in()],
            vec![plugin(
                "broken-ui",
                &[("ui:broken", Some("ui:missing"))],
                (true, &["ui:missing"]),
            )],
        );
        let broken = report
            .plugins
            .iter()
            .find(|resolution| resolution.plugin_id.as_str() == "broken-ui")
            .unwrap();
        assert!(matches!(broken.status, PluginResolutionStatus::Rejected { .. }));
        assert_eq!(report.active.len(), 3);
    }
}
