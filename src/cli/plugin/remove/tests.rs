use super::*;
use rho_harness_core::config::Config;
use std::fs;
use tempfile::tempdir;

#[tokio::test]
async fn test_remove_plugin_in_cargo_bin_deletes_binary() {
    let temp = tempdir().unwrap();
    let config_dir = temp.path().join("config");
    let bin_dir = temp.path().join("bin");
    fs::create_dir_all(&config_dir).unwrap();
    fs::create_dir_all(&bin_dir).unwrap();

    let binary = bin_dir.join("rho-plugin-sample");
    fs::write(&binary, "executable content").unwrap();

    let plugin_cfg = PluginConfig {
        command: Some("rho-plugin-sample".to_string()),
        ..Default::default()
    };
    Config::add_plugin_async(&config_dir, "sample", plugin_cfg.clone())
        .await
        .unwrap();

    let mut plugins = BTreeMap::new();
    plugins.insert("sample".to_string(), plugin_cfg);

    let result = remove_plugin(
        RemovePluginContext {
            config_dir: &config_dir,
            plugins: &plugins,
            keep_binary: false,
            cargo_bin_dir: Some(&bin_dir),
            home_dir: None,
        },
        "sample",
    )
    .await
    .unwrap();

    assert_eq!(result.name, "sample");
    assert_eq!(result.artifact_status, RemovalArtifactStatus::Deleted(binary.clone()));
    assert!(!binary.exists());

    let removed = Config::remove_plugin_async(&config_dir, "sample").await;
    assert!(removed.is_err());
}

#[tokio::test]
async fn test_remove_plugin_keep_binary() {
    let temp = tempdir().unwrap();
    let config_dir = temp.path().join("config");
    let bin_dir = temp.path().join("bin");
    fs::create_dir_all(&config_dir).unwrap();
    fs::create_dir_all(&bin_dir).unwrap();

    let binary = bin_dir.join("rho-plugin-kept");
    fs::write(&binary, "executable content").unwrap();

    let plugin_cfg = PluginConfig {
        command: Some("rho-plugin-kept".to_string()),
        ..Default::default()
    };
    Config::add_plugin_async(&config_dir, "kept", plugin_cfg.clone())
        .await
        .unwrap();

    let mut plugins = BTreeMap::new();
    plugins.insert("kept".to_string(), plugin_cfg);

    let result = remove_plugin(
        RemovePluginContext {
            config_dir: &config_dir,
            plugins: &plugins,
            keep_binary: true,
            cargo_bin_dir: Some(&bin_dir),
            home_dir: None,
        },
        "kept",
    )
    .await
    .unwrap();

    assert_eq!(result.name, "kept");
    assert_eq!(result.artifact_status, RemovalArtifactStatus::Kept(binary.clone()));
    assert!(binary.exists());
}

#[tokio::test]
async fn test_remove_plugin_preserves_external_binary() {
    let temp = tempdir().unwrap();
    let config_dir = temp.path().join("config");
    let bin_dir = temp.path().join("bin");
    let ext_dir = temp.path().join("external");
    fs::create_dir_all(&config_dir).unwrap();
    fs::create_dir_all(&bin_dir).unwrap();
    fs::create_dir_all(&ext_dir).unwrap();

    let ext_binary = ext_dir.join("external-tool");
    fs::write(&ext_binary, "tool content").unwrap();

    let plugin_cfg = PluginConfig {
        path: ext_binary.clone(),
        ..Default::default()
    };
    Config::add_plugin_async(&config_dir, "external", plugin_cfg.clone())
        .await
        .unwrap();

    let mut plugins = BTreeMap::new();
    plugins.insert("external".to_string(), plugin_cfg);

    let result = remove_plugin(
        RemovePluginContext {
            config_dir: &config_dir,
            plugins: &plugins,
            keep_binary: false,
            cargo_bin_dir: Some(&bin_dir),
            home_dir: None,
        },
        "external",
    )
    .await
    .unwrap();

    assert_eq!(result.name, "external");
    assert_eq!(
        result.artifact_status,
        RemovalArtifactStatus::PreservedExternal(ext_binary.clone())
    );
    assert!(ext_binary.exists());
}

#[tokio::test]
async fn test_remove_plugin_missing_binary() {
    let temp = tempdir().unwrap();
    let config_dir = temp.path().join("config");
    let bin_dir = temp.path().join("bin");
    fs::create_dir_all(&config_dir).unwrap();
    fs::create_dir_all(&bin_dir).unwrap();

    let plugin_cfg = PluginConfig {
        command: Some("rho-plugin-ghost".to_string()),
        ..Default::default()
    };
    Config::add_plugin_async(&config_dir, "ghost", plugin_cfg.clone())
        .await
        .unwrap();

    let mut plugins = BTreeMap::new();
    plugins.insert("ghost".to_string(), plugin_cfg);

    let result = remove_plugin(
        RemovePluginContext {
            config_dir: &config_dir,
            plugins: &plugins,
            keep_binary: false,
            cargo_bin_dir: Some(&bin_dir),
            home_dir: None,
        },
        "ghost",
    )
    .await
    .unwrap();

    assert_eq!(result.name, "ghost");
    assert_eq!(result.artifact_status, RemovalArtifactStatus::NotFound);
}

#[tokio::test]
async fn test_remove_plugin_not_configured_returns_error() {
    let temp = tempdir().unwrap();
    let config_dir = temp.path().join("config");
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(config_dir.join("config.toml"), "").unwrap();

    let plugins = BTreeMap::new();
    let result = remove_plugin(
        RemovePluginContext {
            config_dir: &config_dir,
            plugins: &plugins,
            keep_binary: false,
            cargo_bin_dir: None,
            home_dir: None,
        },
        "nonexistent",
    )
    .await;

    assert!(result.is_err());
}

#[test]
fn test_resolve_plugin_key() {
    let mut plugins = BTreeMap::new();
    plugins.insert("rho-plugin-git".to_string(), PluginConfig::default());
    plugins.insert("search".to_string(), PluginConfig::default());

    assert_eq!(resolve_plugin_key("git", &plugins), Some("rho-plugin-git".to_string()));
    assert_eq!(
        resolve_plugin_key("rho-plugin-git", &plugins),
        Some("rho-plugin-git".to_string())
    );
    assert_eq!(resolve_plugin_key("search", &plugins), Some("search".to_string()));
    assert_eq!(
        resolve_plugin_key("rho-plugin-search", &plugins),
        Some("search".to_string())
    );
    assert_eq!(resolve_plugin_key("unknown", &plugins), None);
}
