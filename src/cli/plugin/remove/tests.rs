use super::*;
use rho_harness_core::config::Config;
use std::fs;
use std::path::PathBuf;
use tempfile::tempdir;

struct RemoveFixture {
    _temp: tempfile::TempDir,
    config_dir: PathBuf,
    bin_dir: PathBuf,
}

impl RemoveFixture {
    fn new() -> Self {
        let temp = tempdir().unwrap();
        let config_dir = temp.path().join("config");
        let bin_dir = temp.path().join("bin");
        fs::create_dir_all(&config_dir).unwrap();
        fs::create_dir_all(&bin_dir).unwrap();
        Self {
            _temp: temp,
            config_dir,
            bin_dir,
        }
    }
}

#[tokio::test]
async fn test_remove_plugin_in_cargo_bin_deletes_binary() {
    let f = RemoveFixture::new();
    let binary = f.bin_dir.join("rho-plugin-sample");
    fs::write(&binary, "executable content").unwrap();

    let plugin_cfg = PluginConfig {
        command: Some("rho-plugin-sample".to_string()),
        ..Default::default()
    };
    Config::add_plugin_async(&f.config_dir, "sample", plugin_cfg.clone())
        .await
        .unwrap();
    let plugins = BTreeMap::from([("sample".to_string(), plugin_cfg)]);

    let ctx = RemovePluginContext {
        config_dir: &f.config_dir,
        plugins: &plugins,
        keep_binary: false,
        cargo_bin_dir: Some(&f.bin_dir),
        home_dir: None,
    };
    let result = remove_plugin(ctx, "sample").await.unwrap();

    assert_eq!(result.name, "sample");
    assert_eq!(result.artifact_status, RemovalArtifactStatus::Deleted(binary.clone()));
    assert!(!binary.exists());
    assert!(Config::remove_plugin_async(&f.config_dir, "sample").await.is_err());
}

#[tokio::test]
async fn test_remove_plugin_keep_binary() {
    let f = RemoveFixture::new();
    let binary = f.bin_dir.join("rho-plugin-kept");
    fs::write(&binary, "executable content").unwrap();

    let plugin_cfg = PluginConfig {
        command: Some("rho-plugin-kept".to_string()),
        ..Default::default()
    };
    Config::add_plugin_async(&f.config_dir, "kept", plugin_cfg.clone())
        .await
        .unwrap();
    let plugins = BTreeMap::from([("kept".to_string(), plugin_cfg)]);

    let ctx = RemovePluginContext {
        config_dir: &f.config_dir,
        plugins: &plugins,
        keep_binary: true,
        cargo_bin_dir: Some(&f.bin_dir),
        home_dir: None,
    };
    let result = remove_plugin(ctx, "kept").await.unwrap();

    assert_eq!(result.name, "kept");
    assert_eq!(result.artifact_status, RemovalArtifactStatus::Kept(binary.clone()));
    assert!(binary.exists());
}

async fn setup_external_plugin(f: &RemoveFixture) -> (PathBuf, BTreeMap<String, PluginConfig>) {
    let ext_binary = f._temp.path().join("external/tool");
    fs::create_dir_all(ext_binary.parent().unwrap()).unwrap();
    fs::write(&ext_binary, "tool content").unwrap();
    let plugin_cfg = PluginConfig {
        path: ext_binary.clone(),
        ..Default::default()
    };
    Config::add_plugin_async(&f.config_dir, "external", plugin_cfg.clone())
        .await
        .unwrap();
    (ext_binary, BTreeMap::from([("external".to_string(), plugin_cfg)]))
}

#[tokio::test]
async fn test_remove_plugin_preserves_external_binary() {
    let f = RemoveFixture::new();
    let (ext_binary, plugins) = setup_external_plugin(&f).await;

    let ctx = RemovePluginContext {
        config_dir: &f.config_dir,
        plugins: &plugins,
        keep_binary: false,
        cargo_bin_dir: Some(&f.bin_dir),
        home_dir: None,
    };
    let result = remove_plugin(ctx, "external").await.unwrap();

    assert_eq!(result.name, "external");
    assert_eq!(
        result.artifact_status,
        RemovalArtifactStatus::PreservedExternal(ext_binary.clone())
    );
    assert!(ext_binary.exists());
}

#[tokio::test]
async fn test_remove_plugin_missing_binary() {
    let f = RemoveFixture::new();
    let plugin_cfg = PluginConfig {
        command: Some("rho-plugin-ghost".to_string()),
        ..Default::default()
    };
    Config::add_plugin_async(&f.config_dir, "ghost", plugin_cfg.clone())
        .await
        .unwrap();
    let plugins = BTreeMap::from([("ghost".to_string(), plugin_cfg)]);

    let ctx = RemovePluginContext {
        config_dir: &f.config_dir,
        plugins: &plugins,
        keep_binary: false,
        cargo_bin_dir: Some(&f.bin_dir),
        home_dir: None,
    };
    let result = remove_plugin(ctx, "ghost").await.unwrap();

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
    let ctx = RemovePluginContext {
        config_dir: &config_dir,
        plugins: &plugins,
        keep_binary: false,
        cargo_bin_dir: None,
        home_dir: None,
    };
    assert!(remove_plugin(ctx, "nonexistent").await.is_err());
}

#[test]
fn test_resolve_plugin_key() {
    let plugins = BTreeMap::from([
        ("rho-plugin-git".to_string(), PluginConfig::default()),
        ("search".to_string(), PluginConfig::default()),
    ]);
    let cases = [
        ("git", Some("rho-plugin-git".to_string())),
        ("rho-plugin-git", Some("rho-plugin-git".to_string())),
        ("search", Some("search".to_string())),
        ("rho-plugin-search", Some("search".to_string())),
        ("unknown", None),
    ];
    for (key, expected) in cases {
        assert_eq!(resolve_plugin_key(key, &plugins), expected);
    }
}
