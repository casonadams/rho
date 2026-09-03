use super::supports_tool_result_images;

#[test]
fn providers_that_serialize_tool_result_images() {
    for provider in ["anthropic", "gemini", "google", "chatgpt"] {
        assert!(
            supports_tool_result_images(provider),
            "{provider} should support tool-result images"
        );
    }
}

#[test]
fn providers_that_hard_error_on_tool_result_images() {
    for provider in [
        "openai",
        "openrouter",
        "ollama",
        "local",
        "ollama-cloud",
        "xai",
        "groq",
        "deepseek",
        "mistral",
        "cohere",
        "copilot",
        "antigravity",
        "google-antigravity",
    ] {
        assert!(
            !supports_tool_result_images(provider),
            "{provider} should reject tool-result images"
        );
    }
}

#[test]
fn unknown_providers_default_to_false() {
    for provider in ["my-custom-provider", "", "  ", "ANTROPIC-TYPO"] {
        assert!(
            !supports_tool_result_images(provider),
            "'{provider}' is not a known provider"
        );
    }
}

#[test]
fn provider_names_are_trimmed_and_case_insensitive() {
    assert!(supports_tool_result_images("  Anthropic  "));
    assert!(!supports_tool_result_images("  OpenAI  "));
}
