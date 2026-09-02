//! Interactive CLI login, logout, and terminal OAuth callback handlers.

use crate::auth::AuthStore;
use crate::config::Config;
use crate::error::{AppError, Result};
use async_trait::async_trait;
use rho_core::auth::{DeviceCodeInfo, OAuthLoginCallbacks, SelectOption};
use rho_core::provider::ProviderId;
use rho_engine::auth::perform_oauth_login;
use std::process::Command;
use std::str::FromStr;

pub struct TerminalOAuthCallbacks;

#[async_trait]
impl OAuthLoginCallbacks for TerminalOAuthCallbacks {
    async fn on_auth_url(&self, url: &str, instructions: Option<&str>) -> Result<()> {
        let msg = instructions.unwrap_or("Authenticate in your browser:");
        println!("\n  \x1b[1m{msg}\x1b[0m");
        println!("  URL: \x1b[4;34m{url}\x1b[0m\n");
        let _ = open_url_in_browser(url);
        Ok(())
    }

    async fn on_device_code(&self, info: &DeviceCodeInfo<'_>) -> Result<()> {
        println!(
            "\n  \x1b[1mFirst copy your one-time code:\x1b[0m \x1b[1;36m{}\x1b[0m",
            info.user_code
        );
        println!(
            "  \x1b[1mThen open:\x1b[0m \x1b[4;34m{}\x1b[0m\n",
            info.verification_uri
        );
        let _ = open_url_in_browser(info.verification_uri);
        Ok(())
    }

    async fn on_prompt(&self, message: &str, secret: bool) -> Result<String> {
        if secret {
            prompt_password(message)
        } else {
            prompt_text(message)
        }
    }

    async fn on_select(&self, message: &str, options: &[SelectOption]) -> Result<Option<String>> {
        #[cfg(feature = "ui")]
        {
            let labels: Vec<String> = options
                .iter()
                .map(|o| {
                    if let Some(desc) = &o.description {
                        format!("{} - {}", o.label, desc)
                    } else {
                        o.label.clone()
                    }
                })
                .collect();
            let selection = inquire::Select::new(message, labels)
                .prompt()
                .map_err(|_| AppError::Cancelled("Selection cancelled".to_string()))?;
            for (idx, opt) in options.iter().enumerate() {
                if selection.starts_with(&opt.label) || selection.contains(&opt.label) {
                    return Ok(Some(options[idx].id.clone()));
                }
            }
            Ok(None)
        }
        #[cfg(not(feature = "ui"))]
        {
            println!("{message}");
            for (idx, opt) in options.iter().enumerate() {
                println!("  {}. {}", idx + 1, opt.label);
            }
            Ok(options.first().map(|o| o.id.clone()))
        }
    }

    async fn on_progress(&self, message: &str) -> Result<()> {
        println!("  • {message}");
        Ok(())
    }
}

pub async fn login_provider(provider: Option<&str>, config: &Config, auth_store: &mut AuthStore) -> Result<()> {
    let target = match provider {
        Some(name) => resolve_provider_name(Some(name), &config.provider),
        None => prompt_select_provider(config)?,
    };

    if let Ok(id) = ProviderId::from_str(&target) {
        match id {
            ProviderId::ChatGpt => {
                let callbacks = TerminalOAuthCallbacks;
                let cred = perform_oauth_login(id, &callbacks).await?;
                auth_store.set_credential(id.as_str(), cred)?;
                println!(
                    "Logged in to ChatGPT. Credentials saved to {}",
                    config.auth_file.display()
                );
                return Ok(());
            }
            ProviderId::Copilot => {
                let callbacks = TerminalOAuthCallbacks;
                let cred = perform_oauth_login(id, &callbacks).await?;
                auth_store.set_credential(id.as_str(), cred)?;
                println!(
                    "Logged in to GitHub Copilot. Credentials saved to {}",
                    config.auth_file.display()
                );
                return Ok(());
            }
            ProviderId::Ollama => {
                println!("Ollama runs locally and does not require credentials.");
                return Ok(());
            }
            _ => {}
        }
    }

    let key = prompt_password(&format!("Enter API key for {target}:"))?;
    let key = key.trim();
    if key.is_empty() {
        return Err(AppError::Auth("API key cannot be empty".to_string()));
    }
    auth_store.set_key(&target, key)?;
    crate::repl::interactive::spawn_background_model_refresh(config, auth_store);
    println!("Stored API key for {target}");
    Ok(())
}

fn prompt_select_provider(config: &Config) -> Result<String> {
    let mut options = vec![
        ("chatgpt", "ChatGPT (Plus/Pro/Team/Enterprise - OAuth subscription)"),
        ("anthropic", "Anthropic (Claude 3.5/3.7 - API key)"),
        ("openai", "OpenAI (GPT-4o/o1/o3 - API key)"),
        ("copilot", "GitHub Copilot (Subscription device login)"),
        ("gemini", "Google Gemini (Gemini 2.0 Flash/Pro - API key)"),
        ("antigravity", "Google Antigravity (Gemini Vertex/Cloud - API key)"),
        ("deepseek", "DeepSeek (DeepSeek V3/R1 - API key)"),
        ("openrouter", "OpenRouter (Universal gateway - API key)"),
        ("xai", "xAI Grok (API key)"),
        ("groq", "Groq (Llama fast inference - API key)"),
        ("mistral", "Mistral AI (API key)"),
        ("cohere", "Cohere (Command R+ - API key)"),
        ("ollama", "Ollama (Local models - no login required)"),
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
            .map_err(|_| AppError::Cancelled("Login cancelled".to_string()))?;
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

pub fn logout_provider(provider: Option<&str>, config: &Config, auth_store: &mut AuthStore) -> Result<()> {
    let target = match provider {
        Some(name) => resolve_provider_name(Some(name), &config.provider),
        None => {
            let configured = auth_store.list_configured_providers();
            if configured.is_empty() {
                println!("No stored credentials to remove.");
                return Ok(());
            }
            #[cfg(feature = "ui")]
            {
                inquire::Select::new("Select provider credentials to remove:", configured)
                    .prompt()
                    .map_err(|_| AppError::Cancelled("Logout cancelled".to_string()))?
            }
            #[cfg(not(feature = "ui"))]
            {
                configured.first().cloned().unwrap_or_default()
            }
        }
    };

    auth_store.remove_key(&target)?;
    println!("Removed stored credentials for {target}");
    Ok(())
}

fn open_url_in_browser(url: &str) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        Command::new("open").arg(url).spawn()?;
    }
    #[cfg(target_os = "linux")]
    {
        Command::new("xdg-open").arg(url).spawn()?;
    }
    #[cfg(target_os = "windows")]
    {
        Command::new("cmd").args(["/C", "start", "", url]).spawn()?;
    }
    Ok(())
}

#[cfg(feature = "ui")]
fn prompt_password(prompt: &str) -> Result<String> {
    inquire::Password::new(prompt)
        .with_display_mode(inquire::PasswordDisplayMode::Masked)
        .without_confirmation()
        .prompt()
        .map_err(|_| AppError::Cancelled("Input cancelled".to_string()))
}

#[cfg(not(feature = "ui"))]
fn prompt_password(prompt: &str) -> Result<String> {
    use std::io::BufRead;
    println!("{prompt}");
    let mut buffer = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut buffer)
        .map_err(|e| AppError::Other(e.into()))?;
    Ok(buffer.trim_end_matches(&['\r', '\n'][..]).to_string())
}

#[cfg(feature = "ui")]
fn prompt_text(prompt: &str) -> Result<String> {
    inquire::Text::new(prompt)
        .prompt()
        .map_err(|_| AppError::Cancelled("Input cancelled".to_string()))
}

#[cfg(not(feature = "ui"))]
fn prompt_text(prompt: &str) -> Result<String> {
    prompt_password(prompt)
}

fn resolve_provider_name(requested: Option<&str>, configured: &str) -> String {
    let requested = requested.unwrap_or(configured).trim().to_ascii_lowercase();
    ProviderId::from_str(&requested)
        .map(|id| id.as_str().to_string())
        .unwrap_or(requested)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_names_canonicalize_and_custom_names_are_kept() {
        assert_eq!(resolve_provider_name(Some("Google"), "anthropic"), "gemini");
        assert_eq!(
            resolve_provider_name(Some("google-antigravity"), "anthropic"),
            "antigravity"
        );
        assert_eq!(resolve_provider_name(None, "GROQ"), "groq");
        assert_eq!(resolve_provider_name(Some("acme"), "anthropic"), "acme");
        assert_eq!(resolve_provider_name(Some("Acme Cloud"), "anthropic"), "acme cloud");
    }
}
