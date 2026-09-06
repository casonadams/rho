use super::*;
use crate::cli::plugin::paths::PluginEnvironment;
use std::fs;
use tempfile::tempdir;

#[test]
fn test_format_plugin_table_empty() {
    let output = format_plugin_table(&[]);
    assert_eq!(output, "No plugins configured.\n");
}

fn create_mock_binary(dir: &std::path::Path, name: &str) -> PathBuf {
    let binary = dir.join(name);
    fs::write(&binary, "#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&binary).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&binary, perms).unwrap();
    }
    binary
}

fn sample_listing_plugins() -> BTreeMap<String, PluginConfig> {
    BTreeMap::from([
        (
            "active".to_string(),
            PluginConfig {
                command: Some("rho-plugin-active".to_string()),
                enabled: true,
                ..Default::default()
            },
        ),
        (
            "missing".to_string(),
            PluginConfig {
                command: Some("rho-plugin-missing".to_string()),
                enabled: false,
                ..Default::default()
            },
        ),
    ])
}

#[test]
fn test_collect_plugin_listings_installed_and_missing() {
    let temp = tempdir().unwrap();
    let bin_dir = temp.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    create_mock_binary(&bin_dir, "rho-plugin-active");

    let plugins = sample_listing_plugins();
    let env = PluginEnvironment {
        cargo_bin_dir: Some(&bin_dir),
        home_dir: None,
    };
    let items = collect_plugin_listings(&plugins, env);
    assert_eq!(items.len(), 2);
    let active = items.iter().find(|i| i.name == "active").unwrap();
    assert_eq!(
        (active.status.as_str(), active.managed.as_str(), active.enabled),
        ("Installed (active)", "cargo-bin", true)
    );
    let missing = items.iter().find(|i| i.name == "missing").unwrap();
    assert_eq!(
        (missing.status.as_str(), missing.managed.as_str(), missing.enabled),
        ("Missing", "cargo-bin", false)
    );
}

#[test]
fn test_collect_plugin_listings_external_managed() {
    let temp = tempdir().unwrap();
    let (bin_dir, ext_dir) = (temp.path().join("bin"), temp.path().join("external"));
    fs::create_dir_all(&bin_dir).unwrap();
    fs::create_dir_all(&ext_dir).unwrap();
    let ext_binary = create_mock_binary(&ext_dir, "ext-tool");

    let plugins = BTreeMap::from([(
        "ext".to_string(),
        PluginConfig {
            path: ext_binary,
            ..Default::default()
        },
    )]);
    let items = collect_plugin_listings(
        &plugins,
        PluginEnvironment {
            cargo_bin_dir: Some(&bin_dir),
            home_dir: None,
        },
    );
    assert_eq!(
        (items[0].status.as_str(), items[0].managed.as_str()),
        ("Installed (active)", "system/local")
    );
}

fn sample_listing_items() -> Vec<PluginListingItem> {
    vec![
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
    ]
}

#[test]
fn test_format_plugin_table_rendering() {
    let items = sample_listing_items();
    let output = format_plugin_table(&items);
    for col in [
        "NAME",
        "COMMAND / PATH",
        "MANAGED",
        "STATUS",
        "test-plugin",
        "Installed (active)",
    ] {
        assert!(output.contains(col));
    }
}
