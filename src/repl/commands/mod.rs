pub mod args;
pub mod help;

#[cfg(test)]
mod tests;

pub use args::parse_command_args;
pub use help::print_help;

use crate::auth::AuthStore;
use crate::config::Config;
use crate::error::Result;
use crate::ui::TerminalRenderer;
use std::fmt::Write as _;

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
    SwitchBranch {
        leaf_id: String,
    },
    ForkSession {
        turn_or_node_id: Option<String>,
    },
    CloneSession,
    NameSession {
        name: String,
    },
    ResumeSession {
        session_id: String,
    },
    ExpandedPrompt {
        text: String,
    },
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
    pub session_manager: Option<&'a rho_core::session::SessionManager>,
}

pub const SLASH_COMMANDS: &[&str] = &[
    "/help", "/model", "/skill", "/plugin", "/session", "/compact", "/tree", "/rewind", "/resume", "/fork", "/clone",
    "/name", "/clear", "/login", "/logout", "/exit",
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
            "tree" => {
                if let Some(sm) = ctx.session_manager {
                    let tree = sm.load_tree().await?;
                    if ctx.renderer.has_interactive_ui() && !tree.is_empty() {
                        let entries = crate::ui::interactive::tree_view::build_tree_display(&tree);
                        let choices: Vec<String> = entries
                            .iter()
                            .map(|e| {
                                let active = if e.is_active { " [ACTIVE]" } else { "" };
                                let label = e.label.as_ref().map(|l| format!(" [{l}]")).unwrap_or_default();
                                format!(
                                    "{}{} - {}{}{}",
                                    "  ".repeat(e.depth),
                                    &e.id[..8.min(e.id.len())],
                                    e.preview,
                                    label,
                                    active
                                )
                            })
                            .collect();
                        if let Ok(choice) =
                            inquire::Select::new("Select a tree node to switch branch / inspect:", choices).prompt()
                        {
                            let short_id = choice.split_whitespace().next().unwrap_or("");
                            if let Some(entry) = entries.iter().find(|e| e.id.starts_with(short_id)) {
                                return Ok(Some(CommandResult::SwitchBranch {
                                    leaf_id: entry.id.clone(),
                                }));
                            }
                        }
                        Ok(Some(CommandResult::Continue))
                    } else {
                        let rendered = crate::ui::interactive::tree_view::render_tree_ascii(&tree);
                        ctx.renderer
                            .print_notice(&format!("\nConversation Tree:\n{rendered}\n"));
                        Ok(Some(CommandResult::Continue))
                    }
                } else {
                    Ok(Some(CommandResult::Tree))
                }
            }
            "name" => {
                if parts.len() > 1 {
                    let name = parts[1..].join(" ");
                    Ok(Some(CommandResult::NameSession { name }))
                } else if ctx.renderer.has_interactive_ui() {
                    if let Ok(name) = inquire::Text::new("Session Name:").prompt()
                        && !name.trim().is_empty()
                    {
                        return Ok(Some(CommandResult::NameSession {
                            name: name.trim().to_string(),
                        }));
                    }
                    Ok(Some(CommandResult::Continue))
                } else {
                    ctx.renderer.print_notice("  Usage: /name <session_name>\n");
                    Ok(Some(CommandResult::Continue))
                }
            }
            "fork" => {
                let id = if parts.len() > 1 {
                    Some(parts[1].to_string())
                } else {
                    None
                };
                Ok(Some(CommandResult::ForkSession { turn_or_node_id: id }))
            }
            "resume" => {
                if parts.len() > 1 {
                    let id = parts[1].to_string();
                    Ok(Some(CommandResult::ResumeSession { session_id: id }))
                } else if ctx.renderer.has_interactive_ui() {
                    if let Some(picked) =
                        crate::ui::interactive::session_picker::prompt_session_picker(&ctx.config.sessions_dir)?
                    {
                        Ok(Some(CommandResult::ResumeSession { session_id: picked }))
                    } else {
                        Ok(Some(CommandResult::Continue))
                    }
                } else {
                    ctx.renderer.print_notice("  Usage: /resume <session_id>\n");
                    Ok(Some(CommandResult::Continue))
                }
            }
            "clone" => Ok(Some(CommandResult::CloneSession)),
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
                    let models: Vec<String> = crate::repl::interactive::CURATED_MODELS
                        .iter()
                        .map(|(m, p)| format!("{m} ({p})"))
                        .collect();
                    if let Ok(choice) = inquire::Select::new("Select a model:", models).prompt() {
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
                    if let Some(content) = crate::skills::resolved_content(&skills, &matched.metadata.name) {
                        ctx.renderer.print_notice(&format!(
                            "\n[skill: {} ({})]\n{content}\n",
                            matched.metadata.name, matched.origin
                        ));
                    }
                } else if ctx.renderer.has_interactive_ui() {
                    let choices: Vec<String> = skills
                        .iter()
                        .map(|s| format!("{} - {} ({})", s.metadata.name, s.metadata.description, s.origin))
                        .collect();
                    let selected = match inquire::Select::new("Select a skill to inspect:", choices).prompt() {
                        Ok(choice) => Some(choice.split_whitespace().next().unwrap_or("").to_string()),
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
                let cwd = std::env::current_dir().ok();
                let templates =
                    rho_core::prompts::discover_prompt_templates(Some(&ctx.config.config_dir), cwd.as_deref());
                if let Some(template) = templates.iter().find(|t| t.metadata.name == custom) {
                    let args = &parts[1..];
                    let expanded = template.expand(args);
                    return Ok(Some(CommandResult::ExpandedPrompt { text: expanded }));
                }

                if let Some(commands) = ctx.commands
                    && let Some(cmd) = commands.get(custom)
                {
                    let args = parse_command_args(&trimmed[1 + custom.len()..]);
                    let req = rho_sdk::contract::CommandInvocationRequest {
                        arguments: args,
                        context: rho_sdk::contract::InvocationContext {
                            session_id: ctx.session_id.unwrap_or_default().to_string(),
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
