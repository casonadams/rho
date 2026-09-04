use super::super::{Config, PluginConfig, ProviderConfig};

#[test]
fn rejects_invalid_plugin_configuration() {
    let mut config = Config::default();
    config.plugins.insert(
        "Invalid Name".to_string(),
        PluginConfig {
            path: "plugin".into(),
            command: None,
            args: Vec::new(),
            package: None,
            version: None,
            git: None,
            branch: None,
            tag: None,
            enabled: true,
            replaces: Default::default(),
            config: None,
        },
    );
    assert!(config.validate().is_err());
}

#[test]
fn rejects_invalid_provider_configuration() {
    let collision = ProviderConfig {
        base_url: "https://api.acme.dev/v1".to_string(),
        key_env: None,
    };
    let mut config = Config::default();
    config.providers.insert("anthropic".to_string(), collision.clone());
    assert!(config.validate().is_err());

    config.providers.clear();
    config.providers.insert("Bad Name".to_string(), collision.clone());
    assert!(config.validate().is_err());

    config.providers.clear();
    config.providers.insert("acme".to_string(), collision.clone());
    config.providers.insert(
        "ftp".to_string(),
        ProviderConfig {
            base_url: "ftp://api.acme.dev".to_string(),
            key_env: None,
        },
    );
    assert!(config.validate().is_err());

    config.providers.remove("ftp");
    config.providers.insert(
        "garbage".to_string(),
        ProviderConfig {
            base_url: "not a url".to_string(),
            key_env: None,
        },
    );
    assert!(config.validate().is_err());

    config.providers.remove("garbage");
    config.providers.insert("acme".to_string(), collision);
    config.validate().unwrap();
}

#[test]
fn test_runtime_limit_boundaries() {
    let mut cfg = Config {
        max_turns: 0,
        ..Config::default()
    };
    assert!(cfg.validate().is_err());

    cfg.max_turns = 1;
    cfg.max_output_tokens = Some(0);
    assert!(cfg.validate().is_err());

    cfg.max_output_tokens = Some(1);
    cfg.context_window_messages = 0;
    assert!(cfg.validate().is_err());

    cfg.context_window_messages = 1;
    cfg.compaction_max_bytes = 0;
    assert!(cfg.validate().is_err());

    cfg.compaction_max_bytes = 1;
    assert!(cfg.validate().is_ok());
}
