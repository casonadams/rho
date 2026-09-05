pub fn infer_provider_for_model(model: &str) -> Option<&'static str> {
    let m = model.to_ascii_lowercase();
    if m.starts_with("claude-") {
        Some("anthropic")
    } else if m.starts_with("gpt-") || m.starts_with("o1") || m.starts_with("o3") || m.starts_with("chatgpt-") {
        Some("openai")
    } else if m.starts_with("gemini-") {
        Some("gemini")
    } else if m.starts_with("deepseek-") {
        Some("deepseek")
    } else if m.starts_with("grok-") {
        Some("xai")
    } else if m.starts_with("mistral-") || m.starts_with("codestral-") {
        Some("mistral")
    } else if m.starts_with("llama-") {
        Some("groq")
    } else if m.starts_with("command-") {
        Some("cohere")
    } else if m.contains('/') {
        Some("openrouter")
    } else {
        None
    }
}
