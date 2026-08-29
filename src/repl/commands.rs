use crate::auth::AuthStore;
use crate::config::Config;
use crate::engine::provider::ProviderId;
use crate::error::Result;
use crate::plugin::{CommandRequest, ExtensionContext, ExtensionRegistry};
use crate::ui::TerminalRenderer;
use std::fmt::Write as _;
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
    pub renderer: &'a TerminalRenderer,
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
                print_help(ctx.config, ctx.registry, ctx.renderer);
                Ok(Some(CommandResult::Continue))
            }
            "clear" | "reset" => {
                ctx.renderer.write_output("  [Conversation context reset]\n");
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
                    ctx.renderer.write_output(&format!(
                        "  [Switched model to {} ({})]\n",
                        ctx.config.model, ctx.config.provider
                    ));
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
                        ctx.renderer.write_output(&format!(
                            "  [Switched model to {} ({})]\n",
                            ctx.config.model, ctx.config.provider
                        ));
                        return Ok(Some(CommandResult::ModelChanged {
                            new_model: model_str.to_string(),
                            new_provider: Some(provider_str.to_string()),
                        }));
                    }
                    Ok(Some(CommandResult::Continue))
                }
            }
            "skill" | "skills" => {
                let cwd = std::env::current_dir().ok();
                let skills = crate::skills::resolved_skills(Some(&ctx.config.config_dir), cwd.as_deref());
                let lookup = |name: &str| skills.iter().find(|skill| skill.metadata.name == name).cloned();
                let list = |output: &mut String| {
                    for skill in &skills {
                        let _ = writeln!(
                            output,
                            "    - {}: {} ({})",
                            skill.metadata.name, skill.metadata.description, skill.origin
                        );
                    }
                };
                if parts.len() > 1 {
                    let Some(matched) = lookup(parts[1]) else {
                        let mut output = format!("  Skill '{}' not found. Available skills:\n", parts[1]);
                        list(&mut output);
                        ctx.renderer.write_output(&output);
                        return Ok(Some(CommandResult::Continue));
                    };
                    // Content comes from the resolved record; overrides read
                    // their file, never execute it.
                    if let Some(content) = crate::skills::resolved_content(&skills, &matched.metadata.name) {
                        ctx.renderer.write_output(&format!(
                            "\n[skill: {} ({})]\n{content}\n",
                            matched.metadata.name, matched.origin
                        ));
                    }
                } else if ctx.renderer.has_interactive_ui() {
                    // Interactive selection mirrors the resolved list.
                    let choices: Vec<String> = skills
                        .iter()
                        .map(|s| format!("{} - {} ({})", s.metadata.name, s.metadata.description, s.origin))
                        .collect();
                    let selected = match inquire::Select::new("Select a skill to inspect:", choices).prompt() {
                        Ok(choice) => Some(choice.split_whitespace().next().unwrap_or("").to_string()),
                        // Non-interactive or cancelled prompts fall back to the list.
                        Err(_) => None,
                    };
                    match selected.and_then(|name| lookup(&name)) {
                        Some(matched) => {
                            if let Some(content) = crate::skills::resolved_content(&skills, &matched.metadata.name) {
                                ctx.renderer.write_output(&format!(
                                    "\n[skill: {} ({})]\n{content}\n",
                                    matched.metadata.name, matched.origin
                                ));
                            }
                        }
                        None => {
                            let mut output = String::from("Available skills:\n");
                            list(&mut output);
                            ctx.renderer.write_output(&output);
                        }
                    }
                } else {
                    let mut output = String::from("Available skills:\n");
                    list(&mut output);
                    ctx.renderer.write_output(&output);
                }
                Ok(Some(CommandResult::Continue))
            }
            "plugin" | "plugins" => {
                let cwd = std::env::current_dir().ok();
                let inspection = crate::plugin::inspection::inspect(ctx.config, cwd.as_deref()).await?;
                ctx.renderer.write_output(&inspection.render());
                Ok(Some(CommandResult::Continue))
            }
            "login" => Ok(Some(CommandResult::Login {
                provider: parts.get(1).map(|value| (*value).to_string()),
            })),
            "logout" => Ok(Some(CommandResult::Logout {
                provider: parts.get(1).map(|value| (*value).to_string()),
            })),
            "exit" | "quit" => {
                ctx.renderer.write_output("  Bye!\n");
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
                                ctx.renderer.write_output(&format!("{output}\n"));
                            }
                            return Ok(Some(CommandResult::Continue));
                        }
                        Some(Err(err)) => {
                            ctx.renderer
                                .write_output(&format!("  Error running /{custom}: {err}\n"));
                            return Ok(Some(CommandResult::Continue));
                        }
                        None => {}
                    }
                }
                ctx.renderer.write_output(&format!(
                    "  Unknown command: /{custom}. Type /help for available commands.\n"
                ));
                Ok(Some(CommandResult::Continue))
            }
        }
    }
}

fn print_help(config: &Config, registry: Option<&ExtensionRegistry>, renderer: &TerminalRenderer) {
    let mut output = "\nCommands\n\
  /help                       Show this reference\n\
  /model [model] [provider]   Inspect or switch the model\n\
  /skill [name]               List or inspect skills\n\
  /plugin                     List discovered plugins\n\
  /clear                      Start a new session; preserve history\n\
  /login [provider]           Add API-key or subscription auth\n\
  /logout [provider]          Remove stored provider auth\n\
  /exit                       Exit rho\n"
        .to_string();

    if let Some(registry) = registry {
        let commands = registry.list_commands();
        if !commands.is_empty() {
            output.push_str("\nExtension commands\n");
            for (name, description) in commands {
                let _ = writeln!(output, "  /{name:<26} {description}");
            }
        }
    }

    output.push_str(
        "\nShortcuts\n\
  Tab                         Complete slash commands & skill names\n\
  Ctrl+C                      Cancel the active operation\n\
  Ctrl+D                      Exit at an empty prompt\n\
\nCurrent session\n",
    );
    let _ = writeln!(output, "  Model                       {}", config.model);
    match ProviderId::from_str(&config.provider) {
        Ok(provider) => {
            let _ = writeln!(
                output,
                "  Provider                    {provider} ({})",
                provider.auth_mode_label()
            );
        }
        Err(_) => {
            let _ = writeln!(
                output,
                "  Provider                    {} (unsupported)",
                config.provider
            );
        }
    }
    let approval = if config.auto_approve {
        "auto-approved"
    } else {
        "confirmation required"
    };
    let _ = writeln!(output, "  Changes                     {approval}\n");
    renderer.write_output(&output);
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::plugin::{CommandHandler, Extension, ExtensionCommand};
    use crate::ui::interactive::{InteractiveUi, OutputEvent, UiEvent};
    use tokio::sync::mpsc;

    fn collecting_renderer() -> (TerminalRenderer, mpsc::UnboundedReceiver<UiEvent>) {
        let (ui, events) = InteractiveUi::channel();
        (TerminalRenderer::with_ui(ui), events)
    }

    fn collected_output(events: &mut mpsc::UnboundedReceiver<UiEvent>) -> String {
        std::iter::from_fn(|| events.try_recv().ok())
            .filter_map(|event| match event {
                UiEvent::Output(OutputEvent::Text(text)) => Some(text),
                _ => None,
            })
            .collect()
    }

    #[tokio::test]
    async fn skill_command_lists_resolved_overrides_with_origin() {
        let workspace = std::env::temp_dir().join(format!("skill_cmd_{}", uuid::Uuid::new_v4()));
        let config_dir = workspace.join("config");
        let user_skill_dir = config_dir.join("skills").join("team-notes");
        std::fs::create_dir_all(&user_skill_dir).unwrap();
        std::fs::write(
            user_skill_dir.join("SKILL.md"),
            "---\nname: team-notes\ndescription: User notes workflow\n---\n# Notes\nnever executed\n",
        )
        .unwrap();

        let mut config = Config {
            config_dir,
            ..Config::default()
        };
        let mut auth = AuthStore::default();
        let (renderer, mut events) = collecting_renderer();
        let mut context = SlashCommandContext {
            config: &mut config,
            auth_store: &mut auth,
            registry: None,
            context: None,
            renderer: &renderer,
        };

        let listing = SlashCommandHandler::handle("/skills", &mut context).await.unwrap();
        assert!(matches!(listing, Some(CommandResult::Continue)));
        let output = collected_output(&mut events);
        assert!(
            output.contains("    - team-notes: User notes workflow (user)"),
            "{output}"
        );

        // `/skill team-notes` prints the override's file content verbatim.
        let viewing = SlashCommandHandler::handle("/skill team-notes", &mut context)
            .await
            .unwrap();
        assert!(matches!(viewing, Some(CommandResult::Continue)));
        let viewed = collected_output(&mut events);
        assert!(viewed.contains("[skill: team-notes (user)]"));
        assert!(viewed.contains("# Notes"));
        assert!(viewed.contains("never executed"));

        let _ = std::fs::remove_dir_all(&workspace);
    }

    #[tokio::test]
    async fn skill_command_reports_unknown_names_with_available_skills() {
        let mut config = Config::default();
        let mut auth = AuthStore::default();
        let (renderer, mut events) = collecting_renderer();
        let mut context = SlashCommandContext {
            config: &mut config,
            auth_store: &mut auth,
            registry: None,
            context: None,
            renderer: &renderer,
        };

        let result = SlashCommandHandler::handle("/skill does-not-exist", &mut context)
            .await
            .unwrap();

        assert!(matches!(result, Some(CommandResult::Continue)));
        let output = collected_output(&mut events);
        assert!(output.contains("does-not-exist"));
        assert!(output.contains("Available skills"));
    }

    #[tokio::test]
    async fn help_is_emitted_through_the_renderer() {
        let mut config = Config::default();
        let mut auth = AuthStore::default();
        let (renderer, mut events) = collecting_renderer();
        let mut context = SlashCommandContext {
            config: &mut config,
            auth_store: &mut auth,
            registry: None,
            context: None,
            renderer: &renderer,
        };

        let result = SlashCommandHandler::handle("/help", &mut context).await.unwrap();

        assert!(matches!(result, Some(CommandResult::Continue)));
        let output = collected_output(&mut events);
        assert!(output.contains("/model [model] [provider]"));
        assert!(output.contains("Current session"));
    }

    #[tokio::test]
    async fn login_is_dispatched_without_collecting_credentials() {
        let mut config = Config::default();
        let mut auth = AuthStore::default();
        let (renderer, _) = collecting_renderer();
        let mut context = SlashCommandContext {
            config: &mut config,
            auth_store: &mut auth,
            registry: None,
            context: None,
            renderer: &renderer,
        };
        let result = SlashCommandHandler::handle("/login chatgpt", &mut context)
            .await
            .unwrap();
        assert!(matches!(
            result,
            Some(CommandResult::Login {
                provider: Some(provider)
            }) if provider == "chatgpt"
        ));
    }

    #[tokio::test]
    async fn model_switch_is_emitted_and_updates_configuration() {
        let mut config = Config::default();
        let mut auth = AuthStore::default();
        let (renderer, mut events) = collecting_renderer();
        let mut context = SlashCommandContext {
            config: &mut config,
            auth_store: &mut auth,
            registry: None,
            context: None,
            renderer: &renderer,
        };
        let result = SlashCommandHandler::handle("/model gpt-4o openai", &mut context)
            .await
            .unwrap();

        assert!(matches!(result, Some(CommandResult::ModelChanged { .. })));
        assert_eq!(config.model, "gpt-4o");
        assert_eq!(config.provider, "openai");
        assert!(collected_output(&mut events).contains("Switched model to gpt-4o (openai)"));
    }

    struct EchoHandler;

    #[async_trait::async_trait]
    impl CommandHandler for EchoHandler {
        async fn execute(&self, args: &str, _context: &ExtensionContext) -> Result<String> {
            Ok(format!("extension: {args}"))
        }
    }

    struct EchoExtension;

    #[async_trait::async_trait]
    impl Extension for EchoExtension {
        fn name(&self) -> &str {
            "echo-extension"
        }

        fn register_commands(&self) -> Vec<ExtensionCommand> {
            vec![ExtensionCommand {
                name: "echo".to_string(),
                description: "Echo arguments".to_string(),
                handler: Arc::new(EchoHandler),
            }]
        }
    }

    #[tokio::test]
    async fn extension_command_output_is_emitted_through_the_renderer() {
        let mut config = Config::default();
        let mut auth = AuthStore::default();
        let mut registry = ExtensionRegistry::new();
        registry.register(EchoExtension);
        let extension_context = ExtensionContext::new(std::env::temp_dir(), "session");
        let (renderer, mut events) = collecting_renderer();
        let mut context = SlashCommandContext {
            config: &mut config,
            auth_store: &mut auth,
            registry: Some(&registry),
            context: Some(&extension_context),
            renderer: &renderer,
        };

        let result = SlashCommandHandler::handle("/echo hello", &mut context).await.unwrap();

        assert!(matches!(result, Some(CommandResult::Continue)));
        assert_eq!(collected_output(&mut events), "extension: hello\n");
    }
}
