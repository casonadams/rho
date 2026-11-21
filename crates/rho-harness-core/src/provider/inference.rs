const PREFIX_PROVIDERS: &[(&[&str], &str)] = &[
    (&["claude-", "haiku-", "sonnet-", "opus-"], "anthropic"),
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
    PREFIX_PROVIDERS
        .iter()
        .find(|(prefixes, _)| prefixes.iter().any(|&p| m.starts_with(p)))
        .map(|&(_, provider)| provider)
}
