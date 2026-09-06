const PREFIX_PROVIDERS: &[(&[&str], &str)] = &[
    (&["claude-"], "anthropic"),
    (&["gpt-", "o1", "o3", "chatgpt-"], "openai"),
    (&["gemini-"], "gemini"),
    (&["deepseek-"], "deepseek"),
    (&["grok-"], "xai"),
    (&["mistral-", "codestral-"], "mistral"),
    (&["llama-"], "groq"),
    (&["command-"], "cohere"),
];

pub fn infer_provider_for_model(model: &str) -> Option<&'static str> {
    let m = model.to_ascii_lowercase();
    for &(prefixes, provider) in PREFIX_PROVIDERS {
        if prefixes.iter().any(|&p| m.starts_with(p)) {
            return Some(provider);
        }
    }
    m.contains('/').then_some("openrouter")
}
