use crate::config::Config;
use crate::ui::TerminalRenderer;
use rho_engine::auth::AuthStore;
use rho_engine::engine::AgentEngine;

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
    pub home_dir: Option<&'a std::path::Path>,
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
