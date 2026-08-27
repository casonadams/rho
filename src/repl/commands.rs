use crate::auth::AuthStore;
use crate::config::Config;
use crate::engine::provider::ProviderId;
use crate::error::Result;
use crate::plugin::{CommandRequest, ExtensionContext, ExtensionRegistry};
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

pub struct SlashCommandContext<'a> {
    pub config: &'a mut Config,
    pub auth_store: &'a mut AuthStore,
    pub registry: Option<&'a ExtensionRegistry>,
    pub context: Option<&'a ExtensionContext>,
}

pub const SLASH_COMMANDS: &[&str] = &[
    "/help", "/model", "/skill", "/plugin", "/clear", "/login", "/logout", "/exit",
];

pub struct SlashCommandHandler;

impl SlashCommandHandler {
    pub async fn handle(input: &str, ctx: &mut SlashCommandContext<'_>) -> Result<Option<CommandResult>> {
        let trimmed = input.trim();
        if !trimmed.starts_with('/') {
            return Ok(None);
        }

        let parts: Vec<&str> = trimmed[1..].split_whitespace().collect();
        if parts.is_empty() {
            return Ok(None);
        }

        let cmd_name = parts[0].to_lowercase();
        match cmd_name.as_str() {
            "help" => {
                print_help(ctx.config, ctx.registry);
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
                    ctx.config.model = new_model.clone();
                    if let Some(ref p) = new_provider {
                        ctx.config.provider = p.clone();
                    }
                    println!("  [Switched model to {} ({})]", ctx.config.model, ctx.config.provider);
                    Ok(Some(CommandResult::ModelChanged {
                        new_model,
                        new_provider,
                    }))
                } else {
                    let models = [
                        "gpt-5.6-luna (chatgpt)",
                        "gpt-5.4 (chatgpt)",
                        "claude-sonnet-4-6 (anthropic)",
                        "gpt-4o (openai)",
                        "gemini-2.5-pro (gemini)",
                        "deepseek-reasoner (deepseek)",
                    ];
                    if let Ok(choice) = inquire::Select::new("Select a model:", models.to_vec()).prompt() {
                        let model_str = choice.split_whitespace().next().unwrap_or("");
                        let provider_str = choice.split('(').nth(1).and_then(|s| s.strip_suffix(')')).unwrap_or("");
                        ctx.config.model = model_str.to_string();
                        ctx.config.provider = provider_str.to_string();
                        println!("  [Switched model to {} ({})]", ctx.config.model, ctx.config.provider);
                        return Ok(Some(CommandResult::ModelChanged {
                            new_model: model_str.to_string(),
                            new_provider: Some(provider_str.to_string()),
                        }));
                    }
                    Ok(Some(CommandResult::Continue))
                }
            }
            "skill" | "skills" => {
                let skills = crate::skills::builtin_skills();
                if parts.len() > 1 {
                    let skill_name = parts[1];
                    if let Some(content) = crate::skills::get_builtin_skill_content(skill_name) {
                        println!("\n{content}\n");
                    } else {
                        println!("  Skill '{skill_name}' not found. Available skills:");
                        for s in &skills {
                            println!("    - {}: {}", s.name, s.description);
                        }
                    }
                } else {
                    let choices: Vec<String> = skills
                        .iter()
                        .map(|s| format!("{} - {}", s.name, s.description))
                        .collect();
                    if let Ok(choice) = inquire::Select::new("Select a skill to inspect:", choices).prompt() {
                        let chosen_name = choice.split_whitespace().next().unwrap_or("");
                        if let Some(content) = crate::skills::get_builtin_skill_content(chosen_name) {
                            println!("\n{content}\n");
                        }
                    }
                }
                Ok(Some(CommandResult::Continue))
            }
            "plugin" | "plugins" => {
                let cwd = std::env::current_dir().ok();
                let discovery = crate::plugin::PluginLoader::discover(&ctx.config.config_dir, cwd.as_deref())?;
                println!("Discovered plugins:");
                if discovery.manifests.is_empty() && discovery.binary_plugins.is_empty() {
                    println!("  (No plugins found in ~/.cargo/bin, $PATH, or plugin directories)");
                } else {
                    for (path, manifest) in &discovery.manifests {
                        println!("  - {} v{} ({})", manifest.name, manifest.version, path.display());
                    }
                    for bin in &discovery.binary_plugins {
                        println!("  - [binary] {}", bin.display());
                    }
                }
                Ok(Some(CommandResult::Continue))
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
            custom => {
                if let (Some(reg), Some(ext_ctx)) = (ctx.registry, ctx.context)
                    && reg.has_command(custom)
                {
                    let args_str = if parts.len() > 1 {
                        parts[1..].join(" ")
                    } else {
                        String::new()
                    };
                    let req = CommandRequest {
                        name: custom,
                        args: &args_str,
                    };
                    match reg.dispatch_command(&req, ext_ctx).await {
                        Some(Ok(output)) => {
                            if !output.is_empty() {
                                println!("{output}");
                            }
                            return Ok(Some(CommandResult::Continue));
                        }
                        Some(Err(err)) => {
                            eprintln!("  Error running /{custom}: {err}");
                            return Ok(Some(CommandResult::Continue));
                        }
                        None => {}
                    }
                }
                println!("  Unknown command: /{custom}. Type /help for available commands.");
                Ok(Some(CommandResult::Continue))
            }
        }
    }
}

fn print_help(config: &Config, registry: Option<&ExtensionRegistry>) {
    println!("\nCommands");
    println!("  /help                       Show this reference");
    println!("  /model [model] [provider]   Inspect or switch the model");
    println!("  /skill [name]               List or inspect skills");
    println!("  /plugin                     List discovered plugins");
    println!("  /clear                      Start a new session; preserve history");
    println!("  /login [provider]           Add API-key or subscription auth");
    println!("  /logout [provider]          Remove stored provider auth");
    println!("  /exit                       Exit rho");

    if let Some(reg) = registry {
        let ext_cmds = reg.list_commands();
        if !ext_cmds.is_empty() {
            println!("\nExtension commands");
            for (name, desc) in ext_cmds {
                println!("  /{:<26} {}", name, desc);
            }
        }
    }

    println!("\nShortcuts");
    println!("  Tab                         Complete slash commands & skill names");
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

    #[tokio::test]
    async fn test_handle_help() {
        let mut cfg = Config::default();
        let mut auth = AuthStore::default();
        let mut ctx = SlashCommandContext {
            config: &mut cfg,
            auth_store: &mut auth,
            registry: None,
            context: None,
        };
        let res = SlashCommandHandler::handle("/help", &mut ctx).await.unwrap();
        assert!(matches!(res, Some(CommandResult::Continue)));
    }

    #[tokio::test]
    async fn login_is_dispatched_without_collecting_credentials() {
        let mut cfg = Config::default();
        let mut auth = AuthStore::default();
        let mut ctx = SlashCommandContext {
            config: &mut cfg,
            auth_store: &mut auth,
            registry: None,
            context: None,
        };
        let result = SlashCommandHandler::handle("/login chatgpt", &mut ctx).await.unwrap();
        assert!(matches!(
            result,
            Some(CommandResult::Login {
                provider: Some(provider)
            }) if provider == "chatgpt"
        ));
    }

    #[tokio::test]
    async fn test_handle_model_switch() {
        let mut cfg = Config::default();
        let mut auth = AuthStore::default();
        let mut ctx = SlashCommandContext {
            config: &mut cfg,
            auth_store: &mut auth,
            registry: None,
            context: None,
        };
        let res = SlashCommandHandler::handle("/model gpt-4o openai", &mut ctx)
            .await
            .unwrap();
        assert!(matches!(res, Some(CommandResult::ModelChanged { .. })));
        assert_eq!(cfg.model, "gpt-4o");
        assert_eq!(cfg.provider, "openai");
    }

    #[tokio::test]
    async fn test_handle_skill_inspect() {
        let mut cfg = Config::default();
        let mut auth = AuthStore::default();
        let mut ctx = SlashCommandContext {
            config: &mut cfg,
            auth_store: &mut auth,
            registry: None,
            context: None,
        };
        let res = SlashCommandHandler::handle("/skill plan", &mut ctx).await.unwrap();
        assert!(matches!(res, Some(CommandResult::Continue)));
    }
}
