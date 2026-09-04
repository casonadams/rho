use super::terminal::{open_url_in_browser, prompt_password, prompt_text};
use crate::error::{AppError, Result};
use async_trait::async_trait;
use rho_harness_core::auth::{DeviceCodeInfo, OAuthLoginCallbacks, SelectOption};

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
