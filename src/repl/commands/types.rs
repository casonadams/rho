use crate::config::Config;
use crate::ui::TerminalRenderer;
use rho_engine::auth::AuthStore;
use rho_engine::engine::AgentEngine;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkflowLaunch {
    Start {
        name: String,
        inputs: std::collections::BTreeMap<String, String>,
    },
    Resume {
        run_id: String,
    },
}

impl WorkflowLaunch {
    pub fn label(&self) -> &str {
        match self {
            Self::Start { name, .. } => name,
            Self::Resume { .. } => "resume",
        }
    }

    pub async fn run(self, runner: &rho_engine::tools::WorkflowRunner) -> rho_engine::tools::WorkflowRun {
        match self {
            Self::Start { name, inputs } => runner.run(&name, inputs).await,
            Self::Resume { run_id } => runner.resume(&run_id).await,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandResult {
    Continue,
    ClearContext,
    OpenModelSelector,
    OpenModelsSelector,
    OpenToolsSelector,
    OpenAgentsSelector,
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
    OpenHelpSelector,
    OpenLoginSelector,
    OpenMcpSelector,
    ThinkingChanged {
        level: Option<String>,
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
    RunWorkflow(WorkflowLaunch),
    EnterWorktree {
        branch: Option<String>,
    },
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
