use super::DiscoveredModel;
use super::antigravity::{antigravity_display_name, model_recency_key, sort_models_newest_first};
use super::fetch::ollama_context_from_info;
use super::presets::{default_presets_for, format_context_tokens};

#[test]
fn test_model_recency_key_parsing() {
    assert_eq!(model_recency_key("gemini-3.8-flash"), (3, 8));
    assert_eq!(model_recency_key("claude-sonnet-4-6"), (4, 6));
    assert_eq!(model_recency_key("gpt-5"), (5, 0));
    assert_eq!(model_recency_key("model-without-numbers"), (0, 0));
    assert_eq!(model_recency_key("claude-3-7-sonnet-20250219"), (3, 7));
}

#[test]
fn test_sort_models_newest_first_descending_and_stable() {
    let models = vec![
        DiscoveredModel {
            id: "gemini-2.0-flash".into(),
            name: "Gemini 2.0".into(),
            provider: "gemini".into(),
            description: "".into(),
            context_tokens: None,
        },
        DiscoveredModel {
            id: "gemini-3.8-flash".into(),
            name: "Gemini 3.8".into(),
            provider: "gemini".into(),
            description: "".into(),
            context_tokens: None,
        },
        DiscoveredModel {
            id: "gemini-2.0-pro".into(),
            name: "Gemini 2.0 Pro".into(),
            provider: "gemini".into(),
            description: "".into(),
            context_tokens: None,
        },
    ];

    let sorted = sort_models_newest_first(models);
    let ids: Vec<&str> = sorted.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, vec!["gemini-3.8-flash", "gemini-2.0-flash", "gemini-2.0-pro"]);
}

#[test]
fn test_antigravity_display_name_token_conversion() {
    assert_eq!(antigravity_display_name("gemini-3.8-flash"), "Gemini 3.8 Flash");
    assert_eq!(antigravity_display_name("claude-opus-4-6"), "Claude Opus 4.6");
    assert_eq!(antigravity_display_name("gpt-oss-120b"), "GPT OSS 120b");
}

#[test]
fn test_ollama_context_from_info_key_lookup() {
    let mut map = serde_json::Map::new();
    map.insert(
        "qwen2_5_coder.context_length".into(),
        serde_json::Value::Number(32768.into()),
    );
    assert_eq!(ollama_context_from_info(&map), Some(32768));

    let empty = serde_json::Map::new();
    assert_eq!(ollama_context_from_info(&empty), None);
}

#[test]
fn test_format_context_tokens_megabytes_and_kilobytes() {
    assert_eq!(format_context_tokens(1_000_000), "1M ctx");
    assert_eq!(format_context_tokens(2_000_000), "2M ctx");
    assert_eq!(format_context_tokens(200_000), "200k ctx");
    assert_eq!(format_context_tokens(128_000), "128k ctx");
}

#[test]
fn test_default_presets_for_unknown_provider() {
    let fallback = default_presets_for("my-custom-provider");
    assert_eq!(fallback.len(), 1);
    assert_eq!(fallback[0].id, "my-custom-provider-default");
    assert_eq!(fallback[0].provider, "my-custom-provider");
}
