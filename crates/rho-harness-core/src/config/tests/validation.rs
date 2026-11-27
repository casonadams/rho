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

type ZeroCase = (&'static str, fn(&mut Config));

#[test]
fn rejects_zero_in_every_limit_that_rho_config_rejects() {
    let cases: [ZeroCase; 13] = [
        ("max_output_tokens", |c| c.max_output_tokens = Some(0)),
        ("max_turns", |c| c.max_turns = 0),
        ("context_limit", |c| c.context_limit = Some(0)),
        ("context_window_messages", |c| c.context_window_messages = 0),
        ("compaction_max_bytes", |c| c.compaction_max_bytes = 0),
        ("reserve_tokens", |c| c.reserve_tokens = 0),
        ("keep_recent_tokens", |c| c.keep_recent_tokens = 0),
        ("search_timeout_sec", |c| c.search_timeout_sec = 0),
        ("fetch_timeout_sec", |c| c.fetch_timeout_sec = 0),
        ("fetch_limit", |c| c.fetch_limit = 0),
        ("fetch_max_bytes", |c| c.fetch_max_bytes = 0),
        ("output_max_bytes", |c| c.output_max_bytes = 0),
        ("recap.idle_seconds", |c| c.recap.idle_seconds = 0),
    ];
    Config::default().validate().unwrap();
    for (name, zero) in cases {
        let mut cfg = Config::default();
        zero(&mut cfg);
        let error = cfg.validate().unwrap_err().to_string();
        assert!(error.contains(name), "{name}: {error}");
    }
    let unthrottled = Config {
        search_min_interval_ms: 0,
        ..Config::default()
    };
    unthrottled.validate().unwrap();
}

#[test]
fn search_engine_validation_accepts_and_rejects() {
    let cases = [
        ("nonexistent_engine", None, false),
        ("brave", Some(vec!["brave", "unknown_engine"]), false),
        ("brave", Some(vec!["duckduckgo", "ddg_lite", "yahoo", "exa"]), true),
        ("exa", Some(vec!["brave", "duckduckgo"]), true),
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
    for engine in ["firecrawl", "gemini", "google"] {
        let mut cfg = Config::default();
        cfg.tools.web.search.default = engine.to_string();
        let err = cfg.validate().unwrap_err();
        assert!(
            err.to_string()
                .contains("Supported engines: brave, duckduckgo, yahoo, exa")
        );
    }
}

#[test]
fn search_engine_aliases_name_one_backend_and_the_supported_list_names_each_backend() {
    for (alias, canonical) in [(" Brave ", "brave"), ("ddg", "duckduckgo"), ("DDG_LITE", "duckduckgo")] {
        let engine: crate::config::types::SearchEngine = alias.parse().unwrap();
        assert_eq!(engine.to_string(), canonical, "{alias:?}");
    }
    for name in super::super::SUPPORTED_SEARCH_ENGINES.split(", ") {
        let engine: crate::config::types::SearchEngine = name.parse().unwrap();
        assert_eq!(engine.to_string(), name);
    }
    let error = "bing".parse::<crate::config::types::SearchEngine>().unwrap_err();
    assert_eq!(
        error,
        "Unknown search engine 'bing'. Supported engines: brave, duckduckgo, yahoo, exa"
    );
}
