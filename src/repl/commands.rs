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

pub const SLASH_COMMANDS: &[&str] = &["/help", "/model", "/clear", "/login", "/logout", "/exit"];

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
    println!("\nCommands");
    println!("  /help                       Show this reference");
    println!("  /model [model] [provider]   Inspect or switch the model");
    println!("  /clear                      Start a new session; preserve history");
    println!("  /login [provider]           Add API-key or subscription auth");
    println!("  /logout [provider]          Remove stored provider auth");
    println!("  /exit                       Exit rust-ai");
    println!("\nShortcuts");
    println!("  Tab                         Complete slash commands");
    println!("  Ctrl+C                      Cancel the active operation");
    println!("  Ctrl+D                      Exit at an empty prompt");
    println!("\nCurrent session");
    println!("  Model                       {}", config.model);
    let provider = ProviderId::from_str(&config.provider);
    match provider {
        Ok(provider) => println!(
            "  Provider                    {provider} ({})",
            provider.auth_mode_label()
        ),
        Err(_) => println!("  Provider                    {} (unsupported)", config.provider),
    }
    println!(
        "  Changes                     {}",
        if config.auto_approve {
            "auto-approved"
        } else {
            "confirmation required"
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
