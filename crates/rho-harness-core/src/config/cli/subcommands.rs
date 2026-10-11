use clap::Subcommand;

#[derive(Subcommand, Debug, Clone, PartialEq, Eq)]
pub enum Commands {
    /// Verify an API key or explicitly start subscription OAuth
    Login {
        /// Provider name (e.g. anthropic, openai, openrouter, chatgpt, copilot, claude, antigravity)
        provider: Option<String>,
        /// Read API key from standard input instead of terminal prompt
        #[arg(long)]
        key_stdin: bool,
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
    /// Update rho to the latest release
    Update,
    /// Manage Model Context Protocol (MCP) servers
    Mcp {
        #[command(subcommand)]
        action: Option<McpCommands>,
    },
    /// Join an active terminal collaboration session
    Join {
        /// Invitation ticket URI (rho://...)
        ticket: String,
    },
    /// Manage peer-to-peer collaboration sessions
    Collab {
        #[command(subcommand)]
        action: CollabCommands,
    },
    /// Manage sovereign agent peer mesh execution
    Agent {
        #[command(subcommand)]
        action: AgentCommands,
    },
}

#[derive(Subcommand, Debug, Clone, PartialEq, Eq)]
pub enum AgentCommands {
    /// Serve subagent tasks over P2P mesh
    Serve {
        /// Capability level granted to joining peers: read_only, propose, full
        #[arg(long, default_value = "read_only")]
        cap: String,
        /// Optional Iroh relay URL
        #[arg(long)]
        relay: Option<String>,
        /// Optional bind address (e.g. 127.0.0.1:0 for testing)
        #[arg(long)]
        bind: Option<String>,
    },
    /// Delegate execution to a remote agent worker node
    Run {
        /// Ticket URI (rho-agent://...)
        ticket: String,
        /// Prompt or task instructions for the remote agent
        prompt: String,
        /// Context slice or initial file snippet
        #[arg(long)]
        context: Option<String>,
        /// Subagent role: scout, critic, planner, general
        #[arg(long)]
        role: Option<String>,
        /// Maximum turn limit
        #[arg(long)]
        max_turns: Option<usize>,
    },
}

#[derive(Subcommand, Debug, Clone, PartialEq, Eq)]
pub enum CollabCommands {
    /// Join an active terminal collaboration session
    Join {
        /// Invitation ticket URI (rho://...)
        ticket: String,
    },
}

#[derive(Subcommand, Debug, Clone, PartialEq, Eq)]
pub enum McpCommands {
    /// List configured MCP servers
    #[command(visible_alias = "ls")]
    List,
    /// Test an MCP server connection and capability discovery
    Test {
        /// Server name to test
        name: String,
    },
    /// Log in to an authenticated remote MCP server
    Login {
        /// Server name to authenticate
        name: String,
    },
    /// Add an MCP server to configuration
    Add {
        /// Server name
        name: String,
        /// Command or URL
        target: String,
        /// Optional arguments for stdio command
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
    },
    /// Remove an MCP server from configuration
    #[command(visible_aliases = ["rm", "delete"])]
    Remove {
        /// Server name
        name: String,
    },
}
