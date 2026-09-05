use super::*;
use crate::cli::plugin::paths::PluginEnvironment;
use std::fs;
use tempfile::tempdir;

#[test]
fn test_format_plugin_table_empty() {
    let output = format_plugin_table(&[]);
    assert_eq!(output, "No plugins configured.\n");
}

#[test]
fn test_collect_plugin_listings_installed_and_missing() {
    let temp = tempdir().unwrap();
    let bin_dir = temp.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();

    let binary = bin_dir.join("rho-plugin-active");
    fs::write(&binary, "#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&binary).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&binary, perms).unwrap();
    }

    let mut plugins = BTreeMap::new();
    let active_cfg = PluginConfig {
        command: Some("rho-plugin-active".to_string()),
        enabled: true,
        ..Default::default()
    };
    plugins.insert("active".to_string(), active_cfg);

    let missing_cfg = PluginConfig {
        command: Some("rho-plugin-missing".to_string()),
        enabled: false,
        ..Default::default()
    };
    plugins.insert("missing".to_string(), missing_cfg);

    let items = collect_plugin_listings(
        &plugins,
        PluginEnvironment {
            cargo_bin_dir: Some(&bin_dir),
            home_dir: None,
        },
    );
    assert_eq!(items.len(), 2);

    let active_item = items.iter().find(|i| i.name == "active").unwrap();
    assert_eq!(active_item.status, "Installed (active)");
    assert_eq!(active_item.managed, "cargo-bin");
    assert!(active_item.enabled);

    let missing_item = items.iter().find(|i| i.name == "missing").unwrap();
    assert_eq!(missing_item.status, "Missing");
    assert_eq!(missing_item.managed, "cargo-bin");
    assert!(!missing_item.enabled);
}

#[test]
fn test_collect_plugin_listings_external_managed() {
    let temp = tempdir().unwrap();
    let bin_dir = temp.path().join("bin");
    let ext_dir = temp.path().join("external");
    fs::create_dir_all(&bin_dir).unwrap();
    fs::create_dir_all(&ext_dir).unwrap();

    let ext_binary = ext_dir.join("ext-tool");
    fs::write(&ext_binary, "#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&ext_binary).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&ext_binary, perms).unwrap();
    }

    let mut plugins = BTreeMap::new();
    let ext_cfg = PluginConfig {
        path: ext_binary.clone(),
        ..Default::default()
    };
    plugins.insert("ext".to_string(), ext_cfg);

    let items = collect_plugin_listings(
        &plugins,
        PluginEnvironment {
            cargo_bin_dir: Some(&bin_dir),
            home_dir: None,
        },
    );
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].status, "Installed (active)");
    assert_eq!(items[0].managed, "system/local");
}

#[test]
fn test_format_plugin_table_rendering() {
    let items = vec![
        PluginListingItem {
            name: "test-plugin".to_string(),
            command_or_path: "rho-plugin-test".to_string(),
            resolved_path: Some(PathBuf::from("/bin/rho-plugin-test")),
            status: "Installed (active)".to_string(),
            enabled: true,
            managed: "cargo-bin".to_string(),
        },
        PluginListingItem {
            name: "other".to_string(),
            command_or_path: "/opt/other".to_string(),
            resolved_path: None,
            status: "Missing".to_string(),
            enabled: false,
            managed: "system/local".to_string(),
        },
    ];

    let table = format_plugin_table(&items);
    assert!(table.contains("NAME"));
    assert!(table.contains("COMMAND / PATH"));
    assert!(table.contains("STATUS"));
    assert!(table.contains("ENABLED"));
    assert!(table.contains("MANAGED"));
    assert!(table.contains("test-plugin"));
    assert!(table.contains("Installed (active)"));
    assert!(table.contains("yes"));
    assert!(table.contains("cargo-bin"));
    assert!(table.contains("other"));
    assert!(table.contains("Missing"));
    assert!(table.contains("no"));
    assert!(table.contains("system/local"));
}
