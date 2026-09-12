use clap::Subcommand;

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
    /// Update rho to the latest release
    Update,
    /// Manage Model Context Protocol (MCP) servers
    Mcp {
        #[command(subcommand)]
        action: Option<McpCommands>,
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
