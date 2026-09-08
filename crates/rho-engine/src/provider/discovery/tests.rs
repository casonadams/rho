use super::DiscoveredModel;
use super::antigravity::{antigravity_display_name, model_recency_key, sort_models_newest_first};
use super::fetch::ollama_context_from_info;
use super::presets::{
    chatgpt_codex_models, default_presets_for, format_context_tokens, ollama_cloud_preset_models, openai_preset_models,
};

#[test]
fn test_model_recency_key_parsing() {
    let cases = [
        ("gemini-3.8-flash", (3, 8)),
        ("claude-sonnet-4-6", (4, 6)),
        ("gpt-5", (5, 0)),
        ("model-without-numbers", (0, 0)),
        ("claude-3-7-sonnet-20250219", (3, 7)),
    ];
    for (model, expected) in cases {
        assert_eq!(model_recency_key(model), expected);
    }
}

fn make_discovered_gemini_model(id: &str, name: &str) -> DiscoveredModel {
    DiscoveredModel {
        id: id.into(),
        name: name.into(),
        provider: "gemini".into(),
        description: "".into(),
        context_tokens: None,
    }
}

#[test]
fn test_sort_models_newest_first_descending_and_stable() {
    let models = vec![
        make_discovered_gemini_model("gemini-2.0-flash", "Gemini 2.0"),
        make_discovered_gemini_model("gemini-3.8-flash", "Gemini 3.8"),
        make_discovered_gemini_model("gemini-2.0-pro", "Gemini 2.0 Pro"),
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
fn ollama_cloud_presets_carry_real_context_lengths() {
    let models = ollama_cloud_preset_models();
    assert!(
        models
            .iter()
            .any(|m| m.id == "glm-5.3-flash" && m.context_tokens == Some(1_048_576))
    );
    assert!(models.iter().all(|m| m.context_tokens.is_some()));
}

#[test]
fn chatgpt_codex_presets_include_gpt_6_astra() {
    let astra = chatgpt_codex_models()
        .into_iter()
        .find(|model| model.id == "gpt-6-astra");
    assert_eq!(
        astra.map(|model| (model.name, model.provider, model.description)),
        Some((
            "GPT-6 Astra".into(),
            "chatgpt".into(),
            "372k ctx · deep reasoning".into(),
        ))
    );
}

#[test]
fn openai_presets_include_gpt_6_astra() {
    let astra = openai_preset_models()
        .into_iter()
        .find(|model| model.id == "gpt-6-astra");
    assert_eq!(
        astra.map(|model| (model.name, model.provider, model.description)),
        Some((
            "GPT-6 Astra".into(),
            "openai".into(),
            "1.05M ctx · deep reasoning".into(),
        ))
    );
}

#[test]
fn test_default_presets_for_unknown_provider() {
    let fallback = default_presets_for("my-custom-provider");
    assert_eq!(fallback.len(), 1);
    assert_eq!(fallback[0].id, "my-custom-provider-default");
    assert_eq!(fallback[0].provider, "my-custom-provider");
}
