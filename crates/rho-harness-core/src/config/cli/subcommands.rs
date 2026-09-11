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
    /// Install a plugin package
    Install {
        /// Plugin specifier (e.g. rho-plugin-foo, foo@1.0.0, org/repo, or git URL)
        target: String,
        /// Overwrite existing plugin configuration or binary
        #[arg(long = "force", visible_alias = "replace", default_value_t = false)]
        force: bool,
    },
    /// Update rho or installed plugins
    Update {
        /// Update target: omitted for self-update, 'all' for all plugins, or specific plugin name
        target: Option<String>,
    },
    /// Remove a configured plugin
    #[command(visible_aliases = ["uninstall", "rm"])]
    Remove {
        /// Configured plugin name
        name: String,
        /// Keep the downloaded binary in ~/.cargo/bin instead of deleting it
        #[arg(long = "keep-binary", default_value_t = false)]
        keep_binary: bool,
    },
    /// Manage extensions and plugins
    Plugin {
        #[command(subcommand)]
        action: Option<PluginCommands>,
    },
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

#[derive(Subcommand, Debug, Clone, PartialEq, Eq)]
pub enum PluginCommands {
    /// List installed and discovered plugins
    #[command(visible_alias = "ls")]
    List,
    /// Install a plugin package
    Install {
        /// Plugin specifier (e.g. rho-plugin-foo, foo@1.0.0, org/repo, or git URL)
        target: String,
        /// Overwrite existing plugin configuration or binary
        #[arg(long = "force", visible_alias = "replace", default_value_t = false)]
        force: bool,
    },
    /// Update installed plugins
    Update {
        /// Update target: omitted for all, or specific plugin name
        target: Option<String>,
    },
    /// Remove a configured plugin
    #[command(visible_alias = "rm")]
    Remove {
        /// Configured plugin name
        name: String,
        /// Keep the downloaded binary in ~/.cargo/bin instead of deleting it
        #[arg(long = "keep-binary", default_value_t = false)]
        keep_binary: bool,
    },
    /// Inspect active capability implementations and origins
    Inspect {
        /// Optional capability identifier, such as tool:bash
        capability: Option<String>,
    },
}
