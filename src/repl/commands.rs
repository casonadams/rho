use crate::auth::AuthStore;
use crate::config::Config;
use crate::engine::provider::ProviderId;
use crate::error::Result;
use std::str::FromStr;

pub enum CommandResult {
    Continue,
    ModelChanged {
        new_model: String,
        new_provider: Option<String>,
    },
    ClearContext,
    Login {
        provider: Option<String>,
    },
    Logout {
        provider: Option<String>,
    },
    Exit,
}

pub struct SlashCommandHandler;

impl SlashCommandHandler {
    pub fn handle(input: &str, config: &mut Config, _auth_store: &mut AuthStore) -> Result<Option<CommandResult>> {
        let trimmed = input.trim();
        if !trimmed.starts_with('/') {
            return Ok(None);
        }

        let parts: Vec<&str> = trimmed[1..].split_whitespace().collect();
        if parts.is_empty() {
            return Ok(None);
        }

        match parts[0].to_lowercase().as_str() {
            "help" => {
                print_help(config);
                Ok(Some(CommandResult::Continue))
            }
            "clear" | "reset" => {
                println!("  [Conversation context reset]");
                Ok(Some(CommandResult::ClearContext))
            }
            "model" => {
                if parts.len() > 1 {
                    let new_model = parts[1].to_string();
                    let new_provider = parts.get(2).map(|s| s.to_string());
                    config.model = new_model.clone();
                    if let Some(ref p) = new_provider {
                        config.provider = p.clone();
                    }
                    println!("  [Switched model to {} ({})]", config.model, config.provider);
                    Ok(Some(CommandResult::ModelChanged {
                        new_model,
                        new_provider,
                    }))
                } else {
                    println!("  Active model: {} (provider: {})", config.model, config.provider);
                    Ok(Some(CommandResult::Continue))
                }
            }
            "login" => Ok(Some(CommandResult::Login {
                provider: parts.get(1).map(|value| (*value).to_string()),
            })),
            "logout" => Ok(Some(CommandResult::Logout {
                provider: parts.get(1).map(|value| (*value).to_string()),
            })),
            "exit" | "quit" => {
                println!("  Bye!");
                Ok(Some(CommandResult::Exit))
            }
            unknown => {
                println!("  Unknown command: /{unknown}. Type /help for available commands.");
                Ok(Some(CommandResult::Continue))
            }
        }
    }
}

fn print_help(config: &Config) {
    println!("\n  Available Slash Commands:");
    println!("    /help              Show this help reference");
    println!("    /model [name]      Inspect or change the active model and provider");
    println!("    /clear             Start a fresh v2 session; preserve prior files");
    println!("    /login [provider]  Verify an API key or start subscription OAuth");
    println!("    /logout [provider] Remove only that provider's credentials");
    println!("    /exit              Exit session\n");
    println!("  Authentication:");
    println!("    API key: anthropic, openai, deepseek, gemini, groq, openrouter, xai, mistral, cohere");
    println!("    Subscription OAuth: chatgpt, copilot");
    println!("    Local: ollama\n");
    println!("  Current Config:");
    println!("    Model:    {}", config.model);
    let provider = ProviderId::from_str(&config.provider);
    match provider {
        Ok(provider) => println!("    Provider: {provider} ({})", provider.auth_mode_label()),
        Err(_) => println!("    Provider: {} (unsupported)", config.provider),
    }
    println!(
        "    Approved: {}",
        if config.auto_approve {
            "autonomous"
        } else {
            "confirm mutations"
        }
    );
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handle_help() {
        let mut cfg = Config::default();
        let mut auth = AuthStore::default();
        let res = SlashCommandHandler::handle("/help", &mut cfg, &mut auth).unwrap();
        assert!(matches!(res, Some(CommandResult::Continue)));
    }

    #[test]
    fn login_is_dispatched_without_collecting_credentials() {
        let mut cfg = Config::default();
        let mut auth = AuthStore::default();
        let result = SlashCommandHandler::handle("/login chatgpt", &mut cfg, &mut auth).unwrap();
        assert!(matches!(
            result,
            Some(CommandResult::Login {
                provider: Some(provider)
            }) if provider == "chatgpt"
        ));
    }

    #[test]
    fn test_handle_model_switch() {
        let mut cfg = Config::default();
        let mut auth = AuthStore::default();
        let res = SlashCommandHandler::handle("/model gpt-4o openai", &mut cfg, &mut auth).unwrap();
        assert!(matches!(res, Some(CommandResult::ModelChanged { .. })));
        assert_eq!(cfg.model, "gpt-4o");
        assert_eq!(cfg.provider, "openai");
    }
}
