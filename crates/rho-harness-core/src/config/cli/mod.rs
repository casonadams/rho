use clap::{Parser, Subcommand};

pub mod help;
#[cfg(test)]
mod pi_alignment_tests;
#[cfg(test)]
mod tests;

#[derive(Parser, Debug, Clone)]
#[command(
    name = "rho",
    author,
    version,
    about = "Minimal agentic coding CLI harness",
    after_help = help::CLI_AFTER_HELP
)]
pub struct Cli {
    /// One-shot prompt to execute
    #[arg(short = 'p', long = "prompt")]
    pub prompt: Option<String>,

    /// Model to use for completions
    #[arg(short = 'm', long = "model")]
    pub model: Option<String>,

    /// AI provider: API-key, local, or subscription identity (chatgpt/copilot)
    #[arg(long = "provider")]
    pub provider: Option<String>,

    /// Maximum output tokens per model call (provider default when omitted)
    #[arg(long = "max-output-tokens", env = "AI_MAX_OUTPUT_TOKENS")]
    pub max_output_tokens: Option<u64>,

    /// Maximum model calls per agent run
    #[arg(long = "max-turns", env = "AI_MAX_TURNS")]
    pub max_turns: Option<usize>,

    /// Thinking level: off, minimal, low, medium, high, xhigh, max
    #[arg(long = "thinking", env = "AI_THINKING_LEVEL")]
    pub thinking: Option<String>,

    /// Set session display name
    #[arg(short = 'n', long = "name")]
    pub name: Option<String>,

    /// Export session to HTML or Markdown file and exit
    #[arg(long = "export")]
    pub export: Option<String>,

    /// Resume a version-2 session by ID, including any pending budget checkpoint
    #[arg(long = "resume")]
    pub resume: Option<String>,

    /// Continue the last session in the current working directory
    #[arg(short = 'c', long = "continue", default_value_t = false)]
    pub r#continue: bool,

    /// Browse and select from previous sessions to resume
    #[arg(short = 'r', long = "resume-picker", default_value_t = false)]
    pub resume_picker: bool,

    /// Execution mode: interactive, rpc, or json
    #[arg(long = "mode", default_value = "interactive")]
    pub mode: String,

    /// Trailing prompt messages or files to run
    #[arg(trailing_var_arg = true)]
    pub message: Vec<String>,

    /// Override base system prompt
    #[arg(long = "system-prompt")]
    pub system_prompt: Option<String>,

    /// Additional instructions to append to the system prompt
    #[arg(long = "append-system-prompt")]
    pub append_system_prompt: Option<String>,

    /// Disable discovery of AGENTS.md, CLAUDE.md, and .cursorrules
    #[arg(long = "no-context-files", visible_alias = "nc", default_value_t = false)]
    pub no_context_files: bool,

    /// Subcommand to execute
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug, Clone, PartialEq, Eq)]
pub enum Commands {
    /// Verify an API key or explicitly start subscription OAuth
    Login {
        /// Provider name (e.g. anthropic, openai, openrouter, chatgpt, copilot, claude, antigravity)
        provider: Option<String>,
    },
    /// Log out from an AI provider
    Logout {
        /// Provider name
        provider: Option<String>,
    },
    /// Display or edit configuration
    Config {
        /// Config key to inspect or set
        key: Option<String>,
        /// New value for key
        value: Option<String>,
    },
    /// List live provider models when supported, otherwise curated examples
    Models,
    /// Manage extensions and plugins
    Plugin {
        #[command(subcommand)]
        action: Option<PluginCommands>,
    },
}

#[derive(Subcommand, Debug, Clone, PartialEq, Eq)]
pub enum PluginCommands {
    /// List installed and discovered plugins
    List,
    /// Install and validate a plugin via Cargo
    Install {
        /// Crates.io package name to install
        package: String,
        /// Explicitly authorize replacement of a capability identifier
        #[arg(long = "replace")]
        replaces: Vec<String>,
    },
    /// Remove a configured plugin and uninstall its Cargo package when applicable
    Remove {
        /// Configured plugin name
        name: String,
    },
    /// Inspect active capability implementations and origins
    Inspect {
        /// Optional capability identifier, such as tool:bash
        capability: Option<String>,
    },
}
