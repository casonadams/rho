use super::super::{Config, ProviderConfig};

#[test]
fn rejects_colliding_or_bad_provider_name() {
    let valid = ProviderConfig {
        base_url: "https://api.acme.dev/v1".to_string(),
        key_env: None,
        default_model: None,
    };
    let mut config = Config::default();
    config.providers.insert("anthropic".to_string(), valid.clone());
    assert!(config.validate().is_err());

    config.providers.clear();
    config.providers.insert("Bad Name".to_string(), valid);
    assert!(config.validate().is_err());
}

#[test]
fn rejects_invalid_provider_urls() {
    let mut config = Config::default();
    for bad_url in ["ftp://api.acme.dev", "not a url"] {
        config.providers.clear();
        config.providers.insert(
            "test_provider".to_string(),
            ProviderConfig {
                base_url: bad_url.to_string(),
                key_env: None,
                default_model: None,
            },
        );
        assert!(config.validate().is_err());
    }

    config.providers.clear();
    config.providers.insert(
        "acme".to_string(),
        ProviderConfig {
            base_url: "https://api.acme.dev/v1".to_string(),
            key_env: None,
            default_model: None,
        },
    );
    config.validate().unwrap();
}

#[test]
fn rejects_zero_max_turns_and_output_tokens() {
    let mut cfg = Config {
        max_turns: 0,
        ..Config::default()
    };
    assert!(cfg.validate().is_err());

    cfg.max_turns = 1;
    cfg.max_output_tokens = Some(0);
    assert!(cfg.validate().is_err());
}

#[test]
fn rejects_zero_context_and_compaction_bytes() {
    let mut cfg = Config {
        context_window_messages: 0,
        ..Config::default()
    };
    assert!(cfg.validate().is_err());

    cfg.context_window_messages = 1;
    cfg.compaction_max_bytes = 0;
    assert!(cfg.validate().is_err());

    cfg.compaction_max_bytes = 1;
    assert!(cfg.validate().is_ok());
}

#[test]
fn search_engine_validation_accepts_and_rejects() {
    let cases = [
        ("nonexistent_engine", None, false),
        ("brave", Some(vec!["brave", "unknown_engine"]), false),
        ("brave", Some(vec!["duckduckgo", "ddg_lite", "yahoo"]), true),
    ];
    for (default, fallback, valid) in cases {
        let mut cfg = Config::default();
        cfg.tools.web.search.default = default.to_string();
        if let Some(fallback) = &fallback {
            cfg.tools.web.search.fallback = fallback.iter().map(|engine| engine.to_string()).collect();
        }
        assert_eq!(cfg.validate().is_ok(), valid, "{default} {fallback:?}");
    }
}

#[test]
fn rejects_removed_search_engines() {
    for engine in ["firecrawl", "exa", "gemini", "google"] {
        let mut cfg = Config::default();
        cfg.tools.web.search.default = engine.to_string();
        let err = cfg.validate().unwrap_err();
        assert!(err.to_string().contains("Supported engines: brave, duckduckgo, yahoo"));
    }
}
