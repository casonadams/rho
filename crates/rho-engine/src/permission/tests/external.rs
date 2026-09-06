use crate::permission::has_external_permission_plugin;
use rho_harness_core::config::PluginConfig;
use std::collections::BTreeMap;
use std::path::PathBuf;

#[test]
fn test_detects_external_permission_plugin_by_name() {
    let mut plugins = BTreeMap::new();
    plugins.insert(
        "permission".to_string(),
        PluginConfig {
            enabled: true,
            ..PluginConfig::default()
        },
    );
    assert!(has_external_permission_plugin(&plugins));

    let mut plugins = BTreeMap::new();
    plugins.insert(
        "rho-plugin-permission".to_string(),
        PluginConfig {
            enabled: true,
            ..PluginConfig::default()
        },
    );
    assert!(has_external_permission_plugin(&plugins));
}

#[test]
fn test_ignores_disabled_permission_plugin() {
    let mut plugins = BTreeMap::new();
    plugins.insert(
        "permission".to_string(),
        PluginConfig {
            enabled: false,
            ..PluginConfig::default()
        },
    );
    assert!(!has_external_permission_plugin(&plugins));
}

fn guard_map(cfg: PluginConfig) -> BTreeMap<String, PluginConfig> {
    BTreeMap::from([("custom-guard".to_string(), cfg)])
}

#[test]
fn test_detects_by_command_path_or_package() {
    let by_cmd = guard_map(PluginConfig {
        enabled: true,
        command: Some("/usr/local/bin/rho-plugin-permission".to_string()),
        ..PluginConfig::default()
    });
    assert!(has_external_permission_plugin(&by_cmd));

    let by_path = guard_map(PluginConfig {
        enabled: true,
        path: PathBuf::from("/opt/plugins/rho-plugin-permission"),
        ..PluginConfig::default()
    });
    assert!(has_external_permission_plugin(&by_path));

    let by_pkg = guard_map(PluginConfig {
        enabled: true,
        package: Some("casonadams/rho-plugin-permission".to_string()),
        ..PluginConfig::default()
    });
    assert!(has_external_permission_plugin(&by_pkg));
}

#[test]
fn test_returns_false_for_unrelated_plugins_or_empty() {
    let plugins = BTreeMap::new();
    assert!(!has_external_permission_plugin(&plugins));

    let mut plugins = BTreeMap::new();
    plugins.insert(
        "git-helper".to_string(),
        PluginConfig {
            enabled: true,
            command: Some("git-helper".to_string()),
            ..PluginConfig::default()
        },
    );
    assert!(!has_external_permission_plugin(&plugins));
}
