use super::*;
use rho_sdk::capability::{CAPABILITY_API_VERSION, CapabilityDeclaration, CapabilityManifest, PLUGIN_PROTOCOL_VERSION};

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
