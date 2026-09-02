pub mod help;
#[cfg(test)]
mod skill_colon_tests;
#[cfg(test)]
mod tests;

use crate::config::Config;
use crate::ui::TerminalRenderer;
use help::print_help;
use rho_engine::auth::AuthStore;
use rho_harness_core::error::Result;
use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandResult {
    Continue,
    ClearContext,
    OpenModelSelector,
    ModelChanged {
        new_model: String,
        new_provider: Option<String>,
    },
    ExpandedPrompt {
        text: String,
    },
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
    ResumeSession {
        session_id: String,
    },
    NameSession {
        name: String,
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
    Reload,
    Exit,
}

pub struct SlashCommandContext<'a> {
    pub config: &'a mut Config,
    pub auth_store: &'a mut AuthStore,
    pub renderer: &'a TerminalRenderer,
    pub session_id: Option<&'a str>,
    pub session_manager: Option<&'a rho_harness_core::session::SessionManager>,
}

pub const SLASH_COMMANDS: &[&str] = &[
    "/help", "/model", "/skill", "/plugin", "/session", "/compact", "/tree", "/rewind", "/resume", "/fork", "/clone",
    "/name", "/clear", "/login", "/logout", "/reload", "/export", "/exit",
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
                print_help(ctx.config, ctx.renderer);
                Ok(Some(CommandResult::Continue))
            }
            "clear" | "reset" | "new" => {
                ctx.renderer.print_notice("  [Conversation context reset]\n");
                Ok(Some(CommandResult::ClearContext))
            }
            "thinking" => {
                if parts.len() > 1 {
                    let level = parts[1].to_lowercase();
                    ctx.config.thinking_level = if level == "off" { None } else { Some(level.clone()) };
                    let _ = rho_harness_core::state::AppState::set_last_thinking_level(
                        ctx.config.config_dir.as_path(),
                        ctx.config.thinking_level.as_deref(),
                    );
                    ctx.renderer
                        .print_notice(&format!("  [Thinking level set to {level}]\n"));
                } else {
                    let levels: Vec<String> = crate::repl::interactive::completion::THINKING_LEVELS
                        .iter()
                        .map(|(lvl, desc)| format!("{lvl} - {desc}"))
                        .collect();
                    if let Ok(choice) = inquire::Select::new("Select thinking level:", levels).prompt() {
                        let selected_level = choice.split_whitespace().next().unwrap_or("off");
                        ctx.config.thinking_level = if selected_level == "off" {
                            None
                        } else {
                            Some(selected_level.to_string())
                        };
                        let _ = rho_harness_core::state::AppState::set_last_thinking_level(
                            ctx.config.config_dir.as_path(),
                            ctx.config.thinking_level.as_deref(),
                        );
                        ctx.renderer
                            .print_notice(&format!("  [Thinking level set to {selected_level}]\n"));
                    }
                }
                Ok(Some(CommandResult::Continue))
            }
            "session" => {
                let mut out = String::new();
                let _ = writeln!(out, "\nSession Diagnostics");
                let _ = writeln!(out, "  Model:                       {}", ctx.config.model);
                let _ = writeln!(out, "  Provider:                    {}", ctx.config.provider);
                let window = rho_harness_core::tokens::context_window_size(&ctx.config.model);
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
                if parts.len() > 1 {
                    if let Ok(turn) = parts[1].parse::<usize>() {
                        Ok(Some(CommandResult::Rewind { turn }))
                    } else {
                        ctx.renderer
                            .print_notice("  Usage: /rewind <turn_number> (e.g. /rewind 2)\n");
                        Ok(Some(CommandResult::Continue))
                    }
                } else {
                    ctx.renderer
                        .print_notice("  Usage: /rewind <turn_number> (e.g. /rewind 2)\n");
                    Ok(Some(CommandResult::Continue))
                }
            }
            "fork" => Ok(Some(CommandResult::ForkSession {
                turn_or_node_id: parts.get(1).map(|s| s.to_string()),
            })),
            "clone" => Ok(Some(CommandResult::CloneSession)),
            "name" => {
                if parts.len() > 1 {
                    let name = parts[1..].join(" ");
                    Ok(Some(CommandResult::NameSession { name }))
                } else {
                    ctx.renderer.print_notice("  Usage: /name <session_name>\n");
                    Ok(Some(CommandResult::Continue))
                }
            }
            "resume" => {
                if parts.len() > 1 {
                    Ok(Some(CommandResult::ResumeSession {
                        session_id: parts[1].to_string(),
                    }))
                } else {
                    ctx.renderer.print_notice("  Usage: /resume <session_id>\n");
                    Ok(Some(CommandResult::Continue))
                }
            }
            "model" => {
                if parts.len() > 1 {
                    let model_spec = parts[1];
                    let (provider, model) = if let Some((p, m)) = model_spec.split_once(':') {
                        (p.to_string(), m.to_string())
                    } else if parts.len() > 2 {
                        (parts[2].to_string(), model_spec.to_string())
                    } else {
                        (ctx.config.provider.clone(), model_spec.to_string())
                    };

                    ctx.config.provider = provider.clone();
                    ctx.config.model = model.clone();
                    ctx.renderer.print_notice(&format!(
                        "  [Switched model to {} ({})]\n",
                        ctx.config.model, ctx.config.provider
                    ));
                    Ok(Some(CommandResult::ModelChanged {
                        new_model: model,
                        new_provider: Some(provider),
                    }))
                } else if ctx.renderer.has_interactive_ui() {
                    Ok(Some(CommandResult::OpenModelSelector))
                } else {
                    let discovered = crate::repl::interactive::discover_models(ctx.config, ctx.auth_store);
                    let models: Vec<String> = discovered
                        .iter()
                        .map(|m| format!("{} ({}) - {}", m.id, m.provider, m.description))
                        .collect();
                    if let Ok(choice) = inquire::Select::new("Select a model:", models).prompt() {
                        let model_str = choice.split_whitespace().next().unwrap_or("");
                        let provider_str = choice.split('(').nth(1).and_then(|s| s.split(')').next()).unwrap_or("");
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
                let mut out = String::from("\nConfigured MCP Servers & Plugins:\n");
                if ctx.config.mcp.servers.is_empty() && ctx.config.plugins.is_empty() {
                    out.push_str("  (none configured)\n");
                } else {
                    for (name, server) in &ctx.config.mcp.servers {
                        let _ = writeln!(
                            out,
                            "  - [mcp] {name}: {} (enabled: {})",
                            server.command, server.enabled
                        );
                    }
                    for (name, plugin) in &ctx.config.plugins {
                        let target = plugin
                            .command
                            .as_deref()
                            .map(|c| c.to_string())
                            .unwrap_or_else(|| plugin.path.display().to_string());
                        let _ = writeln!(out, "  - [plugin] {name}: {target} (enabled: {})", plugin.enabled);
                    }
                }
                ctx.renderer.print_notice(&out);
                Ok(Some(CommandResult::Continue))
            }
            "login" => Ok(Some(CommandResult::Login {
                provider: parts.get(1).map(|value| (*value).to_string()),
            })),
            "logout" => Ok(Some(CommandResult::Logout {
                provider: parts.get(1).map(|value| (*value).to_string()),
            })),
            "reload" => Ok(Some(CommandResult::Reload)),
            "export" => Self::export(ctx, &parts).await,
            "exit" | "quit" => {
                ctx.renderer.print_notice("  Bye!\n");
                Ok(Some(CommandResult::Exit))
            }
            custom => {
                let cwd = std::env::current_dir().ok();
                if let Some(skill_name) = custom.strip_prefix("skill:") {
                    let skills = crate::skills::resolved_skills(Some(&ctx.config.config_dir), cwd.as_deref());
                    if let Some(matched) = skills.iter().find(|s| s.metadata.name == skill_name)
                        && let Some(content) = crate::skills::resolved_content(&skills, &matched.metadata.name)
                    {
                        ctx.renderer.print_notice(&format!(
                            "\n[skill: {} ({})]\n{content}\n",
                            matched.metadata.name, matched.origin
                        ));
                        let user_args = parts[1..].join(" ");
                        let effective_prompt = if user_args.is_empty() {
                            format!(
                                "<skill name=\"{}\" location=\"{}\">\n{}\n</skill>",
                                matched.metadata.name, matched.metadata.location, content
                            )
                        } else {
                            format!(
                                "<skill name=\"{}\" location=\"{}\">\n{}\n</skill>\n\nSkill input: {}",
                                matched.metadata.name, matched.metadata.location, content, user_args
                            )
                        };
                        return Ok(Some(CommandResult::ExpandedPrompt { text: effective_prompt }));
                    }
                }

                let templates =
                    rho_harness_core::prompts::discover_prompt_templates(Some(&ctx.config.config_dir), cwd.as_deref());
                if let Some(template) = templates.iter().find(|t| t.metadata.name == custom) {
                    let args = &parts[1..];
                    let expanded = template.expand(args);
                    return Ok(Some(CommandResult::ExpandedPrompt { text: expanded }));
                }

                let plugin_name = custom.strip_prefix("plugin:").unwrap_or(custom);
                if let Some(plugin_cfg) = ctx.config.plugins.get(plugin_name)
                    && plugin_cfg.enabled
                {
                    let working_dir = cwd.unwrap_or_default();
                    let renderer_arc: std::sync::Arc<dyn rho_harness_core::presentation::presenter::Presenter> =
                        std::sync::Arc::new(ctx.renderer.clone());
                    let dispatcher = std::sync::Arc::new(rho_engine::plugin::host::HostDispatcher::new(renderer_arc));
                    if let Ok(daemon) =
                        rho_engine::plugin::daemon::DaemonProcess::spawn(rho_engine::plugin::daemon::DaemonSpawnArgs {
                            name: plugin_name,
                            config: plugin_cfg,
                            working_dir: &working_dir,
                            dispatcher,
                        })
                        .await
                    {
                        let user_args = parts[1..].join(" ");
                        let res = daemon
                            .call(
                                "hook/command",
                                serde_json::json!({
                                    "name": plugin_name,
                                    "args": user_args,
                                }),
                            )
                            .await;
                        if let Ok(resp) = res
                            && let Some(result) = resp.result
                            && let Some(text) = result
                                .get("output")
                                .or_else(|| result.get("message"))
                                .and_then(|v| v.as_str())
                        {
                            ctx.renderer.print_notice(&format!("\n{text}\n"));
                        }
                        return Ok(Some(CommandResult::Continue));
                    }
                }

                ctx.renderer.print_notice(&format!(
                    "  Unknown command: /{custom}. Type /help for available commands.\n"
                ));
                Ok(Some(CommandResult::Continue))
            }
        }
    }

    async fn export(ctx: &mut SlashCommandContext<'_>, parts: &[&str]) -> Result<Option<CommandResult>> {
        let Some(session_id) = ctx.session_id else {
            ctx.renderer.print_notice("  [Export requires an active session]\n");
            return Ok(Some(CommandResult::Continue));
        };
        let Some(session_manager) = ctx.session_manager else {
            ctx.renderer.print_notice("  [Export requires an active session]\n");
            return Ok(Some(CommandResult::Continue));
        };

        let usage = "Usage: /export [html|md] [path]\n";
        let first = parts.get(1).copied();
        let (extension, path_override) = match first {
            None => ("md", None),
            Some(arg) => {
                let lower = arg.to_ascii_lowercase();
                match lower.as_str() {
                    "html" => ("html", parts.get(2).copied()),
                    "md" | "markdown" => ("md", parts.get(2).copied()),
                    other if other.ends_with(".md") => ("md", Some(arg)),
                    other if other.ends_with(".html") || other.ends_with(".htm") => ("html", Some(arg)),
                    other if other.contains('/') => ("md", Some(arg)),
                    _ => {
                        ctx.renderer.print_notice(usage);
                        return Ok(Some(CommandResult::Continue));
                    }
                }
            }
        };

        let tree = session_manager.load_tree().await?;
        let content = match extension {
            "html" => rho_harness_core::session::export::render_html(&tree, session_id),
            _ => rho_harness_core::session::export::render_markdown(&tree, session_id),
        };
        let path = path_override
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| ctx.config.sessions_dir.join(format!("{session_id}.{extension}")));
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, content)?;
        ctx.renderer
            .print_notice(&format!("  [Exported session to {}]\n", path.display()));
        Ok(Some(CommandResult::Continue))
    }
}
