use crate::auth::AuthStore;
use crate::config::Config;
use crate::engine::provider::ProviderId;
use crate::error::Result;
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
    Compact {
        instructions: Option<String>,
    },
    Tree,
    Rewind {
        turn: usize,
    },
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
    pub renderer: &'a TerminalRenderer,
    pub commands:
        Option<&'a std::collections::BTreeMap<String, std::sync::Arc<dyn rho_sdk::contract::CommandCapability>>>,
    pub session_id: Option<&'a str>,
}

pub const SLASH_COMMANDS: &[&str] = &[
    "/help", "/model", "/skill", "/plugin", "/session", "/compact", "/tree", "/rewind", "/clear", "/login", "/logout",
    "/exit",
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
                print_help(ctx.config, ctx.renderer, ctx.commands);
                Ok(Some(CommandResult::Continue))
            }
            "clear" | "reset" => {
                ctx.renderer.print_notice("  [Conversation context reset]\n");
                Ok(Some(CommandResult::ClearContext))
            }
            "session" => {
                let mut out = String::new();
                let _ = writeln!(out, "\nSession Diagnostics");
                let _ = writeln!(out, "  Model:                       {}", ctx.config.model);
                let _ = writeln!(out, "  Provider:                    {}", ctx.config.provider);
                let window = rho_core::tokens::context_window_size(&ctx.config.model);
                let _ = writeln!(out, "  Context Capacity:            {} tokens", window);
                let _ = writeln!(
                    out,
                    "  Reserve Threshold:           {} tokens",
                    ctx.config.reserve_tokens
                );
                let _ = writeln!(
                    out,
                    "  Keep Recent Window:          {} tokens",
                    ctx.config.keep_recent_tokens
                );
                let _ = writeln!(out, "  Auto-Approve:                {}", ctx.config.auto_approve);
                let _ = writeln!(out, "  Max Turns:                   {}", ctx.config.max_turns);
                let _ = writeln!(out, "  Steering Mode:               {}", ctx.config.steering_mode);
                let _ = writeln!(out, "  Follow-up Mode:              {}", ctx.config.follow_up_mode);
                if let Some(id) = ctx.session_id {
                    let _ = writeln!(out, "  Session ID:                  {id}");
                }
                let _ = writeln!(out);
                ctx.renderer.print_notice(&out);
                Ok(Some(CommandResult::Continue))
            }
            "compact" => {
                let instructions = if parts.len() > 1 {
                    Some(parts[1..].join(" "))
                } else {
                    None
                };
                Ok(Some(CommandResult::Compact { instructions }))
            }
            "tree" => Ok(Some(CommandResult::Tree)),
            "rewind" => {
                if parts.len() > 1
                    && let Ok(turn) = parts[1].parse::<usize>()
                {
                    Ok(Some(CommandResult::Rewind { turn }))
                } else {
                    ctx.renderer.print_notice("  Usage: /rewind <turn_number>\n");
                    Ok(Some(CommandResult::Continue))
                }
            }
            "model" => {
                if parts.len() > 1 {
                    let new_model = parts[1].to_string();
                    let new_provider = parts.get(2).map(|s| s.to_string());
                    ctx.config.model = new_model.clone();
                    if let Some(ref p) = new_provider {
                        ctx.config.provider = p.clone();
                    }
                    ctx.renderer.print_notice(&format!(
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
                        ctx.renderer.print_notice(&format!(
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
                        ctx.renderer.print_notice(&output);
                        return Ok(Some(CommandResult::Continue));
                    };
                    // Content comes from the resolved record; overrides read
                    // their file, never execute it.
                    if let Some(content) = crate::skills::resolved_content(&skills, &matched.metadata.name) {
                        ctx.renderer.print_notice(&format!(
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
                                ctx.renderer.print_notice(&format!(
                                    "\n[skill: {} ({})]\n{content}\n",
                                    matched.metadata.name, matched.origin
                                ));
                            }
                        }
                        None => {
                            let mut output = String::from("Available skills:\n");
                            list(&mut output);
                            ctx.renderer.print_notice(&output);
                        }
                    }
                } else {
                    let mut output = String::from("Available skills:\n");
                    list(&mut output);
                    ctx.renderer.print_notice(&output);
                }
                Ok(Some(CommandResult::Continue))
            }
            "plugin" | "plugins" => {
                let cwd = std::env::current_dir().ok();
                let inspection = crate::plugin::inspection::inspect(ctx.config, cwd.as_deref()).await?;
                ctx.renderer.print_notice(&inspection.render());
                Ok(Some(CommandResult::Continue))
            }
            "login" => Ok(Some(CommandResult::Login {
                provider: parts.get(1).map(|value| (*value).to_string()),
            })),
            "logout" => Ok(Some(CommandResult::Logout {
                provider: parts.get(1).map(|value| (*value).to_string()),
            })),
            "exit" | "quit" => {
                ctx.renderer.print_notice("  Bye!\n");
                Ok(Some(CommandResult::Exit))
            }
            custom => {
                if let Some(commands) = ctx.commands
                    && let Some(cmd) = commands.get(custom)
                {
                    let args = parse_command_args(&trimmed[1 + custom.len()..]);
                    let req = rho_sdk::contract::CommandInvocationRequest {
                        arguments: args,
                        context: rho_sdk::contract::InvocationContext {
                            session_id: String::new(),
                            working_directory: std::env::current_dir()
                                .map(|p| p.display().to_string())
                                .unwrap_or_else(|_| ".".to_string()),
                            has_interactive_ui: ctx.renderer.has_interactive_ui(),
                        },
                    };
                    match cmd.invoke(req).await {
                        Ok(response) => {
                            if !response.output.is_empty() {
                                ctx.renderer.print_notice(&format!("{}\n", response.output.trim_end()));
                            }
                        }
                        Err(e) => {
                            ctx.renderer.print_notice(&format!("  Command failed: {e}\n"));
                        }
                    }
                    return Ok(Some(CommandResult::Continue));
                }

                ctx.renderer.print_notice(&format!(
                    "  Unknown command: /{custom}. Type /help for available commands.\n"
                ));
                Ok(Some(CommandResult::Continue))
            }
        }
    }
}

fn parse_command_args(input: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut quote_char = ' ';

    for ch in input.trim().chars() {
        match ch {
            '"' | '\'' if in_quotes && ch == quote_char => {
                in_quotes = false;
            }
            '"' | '\'' if !in_quotes => {
                in_quotes = true;
                quote_char = ch;
            }
            c if c.is_whitespace() && !in_quotes => {
                if !current.is_empty() {
                    args.push(std::mem::take(&mut current));
                }
            }
            c => {
                current.push(c);
            }
        }
    }
    if !current.is_empty() {
        args.push(current);
    }
    args
}

fn print_help(
    config: &Config,
    renderer: &TerminalRenderer,
    commands: Option<&std::collections::BTreeMap<String, std::sync::Arc<dyn rho_sdk::contract::CommandCapability>>>,
) {
    let mut output = "\nCommands\n\
  /help                       Show this reference\n\
  /model [model] [provider]   Inspect or switch the model\n\
  /skill [name]               List or inspect skills\n\
  /plugin                     List discovered plugins\n\
  /session                    Display token capacity and session diagnostics\n\
  /compact [instructions]     Summarize earlier context to free context space\n\
  /tree                       View conversation turn history\n\
  /rewind <turn>              Rewind context to a specific prior turn\n\
  /clear                      Start a new session; preserve history\n\
  /login [provider]           Add API-key or subscription auth\n\
  /logout [provider]          Remove stored provider auth\n\
  /exit                       Exit rho\n"
        .to_string();

    if let Some(commands) = commands
        && !commands.is_empty()
    {
        output.push_str("\nInstalled Plugin Commands\n");
        for (name, cmd) in commands {
            let desc = cmd.descriptor().description;
            let _ = writeln!(output, "  /{:<26} {}", name, desc);
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
    renderer.print_notice(&output);
}

#[cfg(test)]
mod tests {
    use super::*;
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
                UiEvent::Transcript(crate::ui::interactive::TranscriptItem::Notice(text)) => Some(text),
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
            renderer: &renderer,
            commands: None,
            session_id: None,
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
            renderer: &renderer,
            commands: None,
            session_id: None,
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
            renderer: &renderer,
            commands: None,
            session_id: None,
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
            renderer: &renderer,
            commands: None,
            session_id: None,
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
            renderer: &renderer,
            commands: None,
            session_id: None,
        };
        let result = SlashCommandHandler::handle("/model gpt-4o openai", &mut context)
            .await
            .unwrap();

        assert!(matches!(result, Some(CommandResult::ModelChanged { .. })));
        assert_eq!(config.model, "gpt-4o");
        assert_eq!(config.provider, "openai");
        assert!(collected_output(&mut events).contains("Switched model to gpt-4o (openai)"));
    }

    struct MockCommand {
        name: String,
        description: String,
    }

    #[async_trait::async_trait]
    impl rho_sdk::contract::CommandCapability for MockCommand {
        fn descriptor(&self) -> rho_sdk::contract::CommandDescriptor {
            rho_sdk::contract::CommandDescriptor {
                id: format!("command:{}", self.name).parse().unwrap(),
                name: self.name.clone(),
                description: self.description.clone(),
            }
        }

        async fn invoke(
            &self,
            request: rho_sdk::contract::CommandInvocationRequest,
        ) -> std::result::Result<rho_sdk::contract::CommandInvocationResponse, rho_sdk::capability::CapabilityError>
        {
            Ok(rho_sdk::contract::CommandInvocationResponse {
                output: format!("{}: {}", self.name, request.arguments.join(", ")),
                exit_code: 0,
            })
        }
    }

    #[tokio::test]
    async fn dynamic_plugin_command_dispatches_with_arguments() {
        let mut config = Config::default();
        let mut auth = AuthStore::default();
        let (renderer, mut events) = collecting_renderer();
        let mock_cmd: std::sync::Arc<dyn rho_sdk::contract::CommandCapability> = std::sync::Arc::new(MockCommand {
            name: "kiln".to_string(),
            description: "Kiln local memory".to_string(),
        });
        let commands = std::collections::BTreeMap::from([("kiln".to_string(), mock_cmd)]);

        let mut context = SlashCommandContext {
            config: &mut config,
            auth_store: &mut auth,
            renderer: &renderer,
            commands: Some(&commands),
            session_id: None,
        };

        let result = SlashCommandHandler::handle("/kiln fire \"./docs path\" --force", &mut context)
            .await
            .unwrap();

        assert!(matches!(result, Some(CommandResult::Continue)));
        let output = collected_output(&mut events);
        assert!(output.contains("kiln: fire, ./docs path, --force"));
    }

    #[tokio::test]
    async fn session_command_prints_diagnostics() {
        let mut config = Config::default();
        let mut auth = AuthStore::default();
        let (renderer, mut events) = collecting_renderer();
        let mut context = SlashCommandContext {
            config: &mut config,
            auth_store: &mut auth,
            renderer: &renderer,
            commands: None,
            session_id: Some("sess_xyz123"),
        };

        let result = SlashCommandHandler::handle("/session", &mut context).await.unwrap();

        assert!(matches!(result, Some(CommandResult::Continue)));
        let output = collected_output(&mut events);
        assert!(output.contains("Session Diagnostics"));
        assert!(output.contains("Context Capacity:"));
        assert!(output.contains("Session ID:                  sess_xyz123"));
    }

    #[tokio::test]
    async fn compact_tree_and_rewind_commands_return_expected_results() {
        let mut config = Config::default();
        let mut auth = AuthStore::default();
        let (renderer, _) = collecting_renderer();
        let mut context = SlashCommandContext {
            config: &mut config,
            auth_store: &mut auth,
            renderer: &renderer,
            commands: None,
            session_id: Some("sess_1"),
        };

        let compact_res = SlashCommandHandler::handle("/compact preserve error details", &mut context)
            .await
            .unwrap();
        assert!(matches!(
            compact_res,
            Some(CommandResult::Compact {
                instructions: Some(ref s)
            }) if s == "preserve error details"
        ));

        let tree_res = SlashCommandHandler::handle("/tree", &mut context).await.unwrap();
        assert!(matches!(tree_res, Some(CommandResult::Tree)));

        let rewind_res = SlashCommandHandler::handle("/rewind 2", &mut context).await.unwrap();
        assert!(matches!(rewind_res, Some(CommandResult::Rewind { turn: 2 })));
    }
}
