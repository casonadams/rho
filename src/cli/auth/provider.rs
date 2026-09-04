use crate::config::Config;
use crate::error::Result;
use rho_harness_core::provider::ProviderId;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMethod {
    ApiKey,
    OAuth,
}

pub fn oauth_provider_options() -> Vec<(&'static str, &'static str)> {
    vec![
        ("antigravity", "Google Cloud Code Assist"),
        ("chatgpt", "ChatGPT Plus/Pro subscription"),
        ("copilot", "GitHub Copilot subscription"),
        ("openrouter", "OpenRouter account (OAuth PKCE)"),
    ]
}

pub fn api_key_provider_options(config: &Config) -> Vec<(String, String)> {
    let mut options = vec![
        ("anthropic".to_string(), "Claude models".to_string()),
        ("cohere".to_string(), "Command models".to_string()),
        ("deepseek".to_string(), "DeepSeek models".to_string()),
        ("gemini".to_string(), "Gemini models".to_string()),
        ("groq".to_string(), "Fast open-weight inference".to_string()),
        ("mistral".to_string(), "Mistral and Codestral models".to_string()),
        ("ollama-cloud".to_string(), "Hosted open models".to_string()),
        ("openai".to_string(), "GPT and reasoning models".to_string()),
        ("openrouter".to_string(), "Universal model gateway".to_string()),
        ("xai".to_string(), "Grok models".to_string()),
    ];

    for custom_name in config.providers.keys() {
        if !options.iter().any(|(id, _)| id == custom_name) {
            options.push((custom_name.clone(), "Configured provider".to_string()));
        }
    }

    options.sort_by(|a, b| a.0.cmp(&b.0));
    options
}

pub fn prompt_select_auth_method() -> Result<AuthMethod> {
    #[cfg(feature = "ui")]
    {
        let options = vec![
            "API key (Anthropic, OpenAI, Gemini, etc.)",
            "OAuth / Subscription (ChatGPT, Copilot, Antigravity, OpenRouter)",
        ];
        let selection = inquire::Select::new("Select authentication method:", options)
            .prompt()
            .map_err(|_| crate::error::AppError::Cancelled("Login cancelled".to_string()))?;

        if selection.starts_with("API key") {
            Ok(AuthMethod::ApiKey)
        } else {
            Ok(AuthMethod::OAuth)
        }
    }
    #[cfg(not(feature = "ui"))]
    {
        println!("Select authentication method:");
        println!("  1. API key");
        println!("  2. OAuth / Subscription");
        Ok(AuthMethod::ApiKey)
    }
}

pub fn prompt_select_oauth_provider() -> Result<String> {
    let options = oauth_provider_options();
    #[cfg(feature = "ui")]
    {
        let items: Vec<String> = options.iter().map(|(id, desc)| format!("{id:<14} [{desc}]")).collect();
        let selection = inquire::Select::new("Select provider to log in:", items)
            .prompt()
            .map_err(|_| crate::error::AppError::Cancelled("Login cancelled".to_string()))?;
        let selected_id = selection.split_whitespace().next().unwrap_or("antigravity");
        Ok(selected_id.to_string())
    }
    #[cfg(not(feature = "ui"))]
    {
        println!("Available OAuth providers:");
        for (id, desc) in &options {
            println!("  - {id:<14} ({desc})");
        }
        Ok("antigravity".to_string())
    }
}

pub fn prompt_select_api_key_provider(config: &Config) -> Result<String> {
    let options = api_key_provider_options(config);
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
        println!("Available API key providers:");
        for (id, desc) in &options {
            println!("  - {id:<14} ({desc})");
        }
        Ok("anthropic".to_string())
    }
}

pub fn prompt_auth_method(provider_label: &str) -> Result<AuthMethod> {
    #[cfg(feature = "ui")]
    {
        let options = vec![
            "OAuth (sign in with browser to generate API key)",
            "API key (enter manually)",
        ];
        let selection = inquire::Select::new(&format!("Select authentication method for {provider_label}:"), options)
            .prompt()
            .map_err(|_| crate::error::AppError::Cancelled("Login cancelled".to_string()))?;

        if selection.starts_with("OAuth") {
            Ok(AuthMethod::OAuth)
        } else {
            Ok(AuthMethod::ApiKey)
        }
    }
    #[cfg(not(feature = "ui"))]
    {
        println!("Select authentication method for {provider_label}:");
        println!("  1. OAuth (sign in with browser to generate API key)");
        println!("  2. API key (enter manually)");
        Ok(AuthMethod::OAuth)
    }
}

pub fn resolve_provider_name(requested: Option<&str>, configured: &str) -> String {
    let requested = requested.unwrap_or(configured).trim().to_ascii_lowercase();
    ProviderId::from_str(&requested)
        .map(|id| id.as_str().to_string())
        .unwrap_or(requested)
}
