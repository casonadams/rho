use crate::config::Config;
use crate::error::Result;
use rho_harness_core::provider::ProviderId;
use std::str::FromStr;

pub fn prompt_select_provider(config: &Config) -> Result<String> {
    let mut options = vec![
        ("chatgpt", "ChatGPT (Plus/Pro/Team/Enterprise - OAuth subscription)"),
        ("anthropic", "Anthropic (Claude 3.5/3.7 - API key)"),
        ("openai", "OpenAI (GPT-4o/o1/o3 - API key)"),
        ("copilot", "GitHub Copilot (Subscription device login)"),
        ("gemini", "Google Gemini (Gemini 2.0 Flash/Pro - API key)"),
        (
            "antigravity",
            "Google Antigravity (Gemini/Claude via Google - subscription OAuth)",
        ),
        ("deepseek", "DeepSeek (DeepSeek V3/R1 - API key)"),
        ("openrouter", "OpenRouter (Universal gateway - API key)"),
        ("xai", "xAI Grok (API key)"),
        ("groq", "Groq (Llama fast inference - API key)"),
        ("mistral", "Mistral AI (API key)"),
        ("cohere", "Cohere (Command R+ - API key)"),
        ("ollama-cloud", "Ollama Cloud (Hosted models - API key)"),
        ("local", "Local / Ollama (Offline models - no login required)"),
    ];

    for custom_name in config.providers.keys() {
        if !options.iter().any(|(id, _)| id == custom_name) {
            options.push((custom_name.as_str(), "Custom provider from config.toml"));
        }
    }

    #[cfg(feature = "ui")]
    {
        let items: Vec<String> = options.iter().map(|(id, desc)| format!("{id:<14} [{desc}]")).collect();
        let selection = inquire::Select::new("Select provider to configure:", items)
            .prompt()
            .map_err(|_| crate::error::AppError::Cancelled("Login cancelled".to_string()))?;
        let selected_id = selection.split_whitespace().next().unwrap_or("anthropic");
        Ok(selected_id.to_string())
    }

    #[cfg(not(feature = "ui"))]
    {
        println!("Available providers:");
        for (id, desc) in &options {
            println!("  - {id:<14} ({desc})");
        }
        Ok("anthropic".to_string())
    }
}

pub fn resolve_provider_name(requested: Option<&str>, configured: &str) -> String {
    let requested = requested.unwrap_or(configured).trim().to_ascii_lowercase();
    ProviderId::from_str(&requested)
        .map(|id| id.as_str().to_string())
        .unwrap_or(requested)
}
