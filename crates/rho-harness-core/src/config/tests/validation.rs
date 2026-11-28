use super::super::{Config, ProviderConfig};
use test_case::test_case;

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

#[test_case("ftp://api.acme.dev"; "non http scheme")]
#[test_case("not a url"; "not a url")]
fn rejects_invalid_provider_urls(bad_url: &str) {
    let mut config = Config::default();
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

#[test]
fn accepts_https_provider_urls() {
    let mut config = Config::default();
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

#[test_case("max_output_tokens", |c| c.max_output_tokens = Some(0); "max output tokens")]
#[test_case("max_turns", |c| c.max_turns = 0; "max turns")]
#[test_case("context_limit", |c| c.context_limit = Some(0); "context limit")]
#[test_case("context_window_messages", |c| c.context_window_messages = 0; "context window messages")]
#[test_case("compaction_max_bytes", |c| c.compaction_max_bytes = 0; "compaction max bytes")]
#[test_case("reserve_tokens", |c| c.reserve_tokens = 0; "reserve tokens")]
#[test_case("keep_recent_tokens", |c| c.keep_recent_tokens = 0; "keep recent tokens")]
#[test_case("search_timeout_sec", |c| c.search_timeout_sec = 0; "search timeout sec")]
#[test_case("fetch_timeout_sec", |c| c.fetch_timeout_sec = 0; "fetch timeout sec")]
#[test_case("fetch_limit", |c| c.fetch_limit = 0; "fetch limit")]
#[test_case("fetch_max_bytes", |c| c.fetch_max_bytes = 0; "fetch max bytes")]
#[test_case("output_max_bytes", |c| c.output_max_bytes = 0; "output max bytes")]
#[test_case("recap.idle_seconds", |c| c.recap.idle_seconds = 0; "recap idle seconds")]
fn rejects_zero_in_every_limit_that_rho_config_rejects(name: &str, zero: fn(&mut Config)) {
    let mut cfg = Config::default();
    zero(&mut cfg);
    let error = cfg.validate().unwrap_err().to_string();
    assert!(error.contains(name), "{error}");
}

#[test]
fn default_and_unthrottled_search_configs_validate() {
    Config::default().validate().unwrap();
    let unthrottled = Config {
        search_min_interval_ms: 0,
        ..Config::default()
    };
    unthrottled.validate().unwrap();
}

#[test_case("nonexistent_engine", None, false; "unknown default")]
#[test_case("brave", Some(&["brave", "unknown_engine"]), false; "unknown fallback")]
#[test_case("brave", Some(&["duckduckgo", "ddg_lite", "yahoo", "exa"]), true; "aliased fallbacks")]
#[test_case("exa", Some(&["brave", "duckduckgo"]), true; "exa default")]
fn search_engine_validation_accepts_and_rejects(default: &str, fallback: Option<&[&str]>, valid: bool) {
    let mut cfg = Config::default();
    cfg.tools.web.search.default = default.to_string();
    if let Some(fallback) = fallback {
        cfg.tools.web.search.fallback = fallback.iter().map(|engine| engine.to_string()).collect();
    }
    assert_eq!(cfg.validate().is_ok(), valid);
}

#[test_case("firecrawl"; "firecrawl")]
#[test_case("gemini"; "gemini")]
#[test_case("google"; "google")]
fn rejects_removed_search_engines(engine: &str) {
    let mut cfg = Config::default();
    cfg.tools.web.search.default = engine.to_string();
    let err = cfg.validate().unwrap_err();
    assert!(
        err.to_string()
            .contains("Supported engines: brave, duckduckgo, yahoo, exa")
    );
}

#[test_case(" Brave ", "brave"; "padded capitalized brave")]
#[test_case("ddg", "duckduckgo"; "ddg")]
#[test_case("DDG_LITE", "duckduckgo"; "uppercase ddg lite")]
fn search_engine_aliases_name_one_backend(alias: &str, canonical: &str) {
    let engine: crate::config::types::SearchEngine = alias.parse().unwrap();
    assert_eq!(engine.to_string(), canonical);
}

#[test]
fn the_supported_search_engine_list_names_each_backend() {
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
