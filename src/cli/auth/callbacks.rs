use super::terminal::{open_url_in_browser_async, prompt_password, prompt_text};
use crate::error::{AppError, Result};
use async_trait::async_trait;
use rho_harness_core::auth::{DeviceCodeInfo, OAuthLoginCallbacks, SelectOption};

pub struct TerminalOAuthCallbacks;

async fn prompt_ui_select(message: &str, options: &[SelectOption]) -> Result<Option<String>> {
    if options.is_empty() {
        return Ok(None);
    }
    println!("\n{message}");
    for (idx, opt) in options.iter().enumerate() {
        if let Some(desc) = &opt.description {
            println!("  {}. {} - {desc}", idx + 1, opt.label);
        } else {
            println!("  {}. {}", idx + 1, opt.label);
        }
    }
    use std::io::Write;
    print!("Enter selection [1-{}]: ", options.len());
    std::io::stdout().flush().ok();
    let mut input = String::new();
    std::io::stdin()
        .read_line(&mut input)
        .map_err(|e| AppError::Other(e.into()))?;
    if let Ok(idx) = input.trim().parse::<usize>()
        && idx >= 1
        && idx <= options.len()
    {
        Ok(Some(options[idx - 1].id.clone()))
    } else {
        Ok(None)
    }
}

#[async_trait]
impl OAuthLoginCallbacks for TerminalOAuthCallbacks {
    async fn on_auth_url(&self, url: &str, instructions: Option<&str>) -> Result<()> {
        let msg = instructions.unwrap_or("Authenticate in your browser:");
        println!("\n  \x1b[1m{msg}\x1b[0m");
        println!("  URL: \x1b[4;34m{url}\x1b[0m\n");
        let _ = open_url_in_browser_async(url).await;
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
        let _ = open_url_in_browser_async(info.verification_uri).await;
        Ok(())
    }

    async fn on_prompt(&self, message: &str, secret: bool) -> Result<String> {
        let msg = message.to_string();
        tokio::task::spawn_blocking(move || {
            if secret {
                prompt_password(&msg)
            } else {
                prompt_text(&msg)
            }
        })
        .await
        .map_err(|e| AppError::Other(e.into()))?
    }

    async fn on_select(&self, message: &str, options: &[SelectOption]) -> Result<Option<String>> {
        prompt_ui_select(message, options).await
    }

    async fn on_progress(&self, message: &str) -> Result<()> {
        println!("  • {message}");
        Ok(())
    }
}
