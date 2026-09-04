pub mod args;
mod custom;
mod export;
pub mod help;
mod model;
mod plugin;
mod session;
mod skill;
mod thinking;

#[cfg(test)]
mod skill_colon_tests;
#[cfg(test)]
mod tests;

use crate::config::Config;
use crate::ui::TerminalRenderer;
use help::print_help;
use rho_engine::auth::AuthStore;
use rho_engine::engine::AgentEngine;
use rho_harness_core::error::Result;

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
    OpenTreeSelector,
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
    OpenSessionSelector,
    OpenSettingsSelector,
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
    pub engine: Option<&'a AgentEngine>,
}

pub const SLASH_COMMANDS: &[&str] = &[
    "/help",
    "/settings",
    "/model",
    "/resume",
    "/thinking",
    "/skill",
    "/plugin",
    "/session",
    "/compact",
    "/tree",
    "/rewind",
    "/fork",
    "/clone",
    "/name",
    "/new",
    "/clear",
    "/login",
    "/logout",
    "/reload",
    "/export",
    "/exit",
    "/quit",
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
                ctx.renderer.print_status("Context cleared");
                Ok(Some(CommandResult::ClearContext))
            }
            "thinking" => thinking::handle_thinking(ctx, &parts),
            "session" => {
                session::handle_session(ctx);
                Ok(Some(CommandResult::Continue))
            }
            "settings" => {
                if ctx.renderer.has_interactive_ui() {
                    Ok(Some(CommandResult::OpenSettingsSelector))
                } else {
                    ctx.renderer.print_notice("  Settings: thinking blocks, tool outputs\n");
                    Ok(Some(CommandResult::Continue))
                }
            }
            "compact" => {
                let instructions = parts.get(1..).filter(|s| !s.is_empty()).map(|s| s.join(" "));
                Ok(Some(CommandResult::Compact { instructions }))
            }
            "tree" => {
                if ctx.renderer.has_interactive_ui() {
                    Ok(Some(CommandResult::OpenTreeSelector))
                } else {
                    Ok(Some(CommandResult::Tree))
                }
            }
            "rewind" => match parts.get(1).and_then(|p| p.parse::<usize>().ok()) {
                Some(turn) => Ok(Some(CommandResult::Rewind { turn })),
                None => {
                    ctx.renderer
                        .print_notice("  Usage: /rewind <turn_number> (e.g. /rewind 2)\n");
                    Ok(Some(CommandResult::Continue))
                }
            },
            "fork" => Ok(Some(CommandResult::ForkSession {
                turn_or_node_id: parts.get(1).map(|s| s.to_string()),
            })),
            "clone" => Ok(Some(CommandResult::CloneSession)),
            "name" => match parts.get(1..) {
                Some(slice) if !slice.is_empty() => Ok(Some(CommandResult::NameSession { name: slice.join(" ") })),
                _ => {
                    ctx.renderer.print_notice("  Usage: /name <session_name>\n");
                    Ok(Some(CommandResult::Continue))
                }
            },
            "resume" => match parts.get(1) {
                Some(id) => Ok(Some(CommandResult::ResumeSession {
                    session_id: (*id).to_string(),
                })),
                None if ctx.renderer.has_interactive_ui() => Ok(Some(CommandResult::OpenSessionSelector)),
                None => {
                    ctx.renderer.print_notice("  Usage: /resume <session_id>\n");
                    Ok(Some(CommandResult::Continue))
                }
            },
            "model" => model::handle_model(ctx, &parts),
            "skill" | "skills" => skill::handle_skill(ctx, &parts),
            "plugin" | "plugins" => Ok(plugin::handle_plugins(ctx)),
            "login" => Ok(Some(CommandResult::Login {
                provider: parts.get(1).map(|value| (*value).to_string()),
            })),
            "logout" => Ok(Some(CommandResult::Logout {
                provider: parts.get(1).map(|value| (*value).to_string()),
            })),
            "reload" => Ok(Some(CommandResult::Reload)),
            "export" => export::handle_export(ctx, &parts).await,
            "exit" | "quit" => {
                ctx.renderer.print_notice("  Bye!\n");
                Ok(Some(CommandResult::Exit))
            }
            custom => custom::handle_custom(ctx, custom, &parts).await,
        }
    }
}
