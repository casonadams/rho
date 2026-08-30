use clap::{Parser, Subcommand};

#[derive(Parser, Debug, Clone)]
#[command(
    name = "rho",
    author,
    version,
    about = "Minimal agentic coding CLI harness",
    after_help = "Authentication:\n  API key: anthropic, openai, deepseek, gemini, groq, openrouter, xai, mistral, cohere\n  Subscription OAuth: chatgpt, copilot (explicit login required)\n  Local: ollama\n\nContext defaults:\n  AI_CONTEXT_WINDOW_MESSAGES=24\n  AI_COMPACTION_MAX_BYTES=8192"
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

    /// Automatically approve operations that are normally approval-required, including outside-workspace writes and mutating or uncertain Bash calls
    #[arg(short = 'y', long = "auto-approve", default_value_t = false)]
    pub auto_approve: bool,

    /// Resume a version-2 session by ID, including any pending budget checkpoint
    #[arg(long = "resume")]
    pub resume: Option<String>,

    /// Continue the last session in the current working directory
    #[arg(short = 'c', long = "continue", default_value_t = false)]
    pub r#continue: bool,

    /// Subcommand to execute
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug, Clone, PartialEq, Eq)]
pub enum Commands {
    /// Verify an API key or explicitly start ChatGPT/Copilot subscription OAuth
    Login {
        /// Provider name (e.g. anthropic, openai, openrouter, chatgpt, copilot)
        provider: Option<String>,
    },
    /// Log out from an AI provider
    Logout {
        /// Provider name
        provider: Option<String>,
    },
    /// Set or remove an Ollama Cloud API key (used to show usage for :cloud models)
    Auth {
        /// Subcommand: set or remove
        #[command(subcommand)]
        action: Option<AuthCommands>,
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
pub enum AuthCommands {
    /// Store an Ollama Cloud API key for showing :cloud usage
    Set,
    /// Remove the stored Ollama Cloud API key
    Remove,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_parsing_prompt() {
        let args = vec!["rho", "-p", "fix bug in auth", "-y"];
        let cli = Cli::try_parse_from(args).unwrap();
        assert_eq!(cli.prompt.as_deref(), Some("fix bug in auth"));
        assert!(cli.auto_approve);
    }

    #[test]
    fn test_cli_parsing_subcommand() {
        let args = vec!["rho", "login", "anthropic"];
        let cli = Cli::try_parse_from(args).unwrap();
        assert_eq!(
            cli.command,
            Some(Commands::Login {
                provider: Some("anthropic".to_string())
            })
        );
    }

    #[test]
    fn test_cli_parsing_plugin_subcommands() {
        let cli = Cli::try_parse_from(["rho", "plugin", "list"]).unwrap();
        assert_eq!(
            cli.command,
            Some(Commands::Plugin {
                action: Some(PluginCommands::List)
            })
        );

        let cli = Cli::try_parse_from(["rho", "plugin", "install", "rho-plugin-git"]).unwrap();
        assert_eq!(
            cli.command,
            Some(Commands::Plugin {
                action: Some(PluginCommands::Install {
                    package: "rho-plugin-git".to_string(),
                    replaces: Vec::new()
                })
            })
        );

        let cli =
            Cli::try_parse_from(["rho", "plugin", "install", "rho-plugin-shell", "--replace", "tool:bash"]).unwrap();
        assert_eq!(
            cli.command,
            Some(Commands::Plugin {
                action: Some(PluginCommands::Install {
                    package: "rho-plugin-shell".to_string(),
                    replaces: vec!["tool:bash".to_string()]
                })
            })
        );

        let cli = Cli::try_parse_from(["rho", "plugin", "remove", "git"]).unwrap();
        assert_eq!(
            cli.command,
            Some(Commands::Plugin {
                action: Some(PluginCommands::Remove {
                    name: "git".to_string()
                })
            })
        );

        let cli = Cli::try_parse_from(["rho", "plugin", "inspect", "tool:bash"]).unwrap();
        assert_eq!(
            cli.command,
            Some(Commands::Plugin {
                action: Some(PluginCommands::Inspect {
                    capability: Some("tool:bash".to_string())
                })
            })
        );
    }

    #[test]
    fn test_cli_parses_runtime_limits() {
        let cli = Cli::try_parse_from(["rho", "--max-output-tokens", "8192", "--max-turns", "12"]).unwrap();
        assert_eq!(cli.max_output_tokens, Some(8192));
        assert_eq!(cli.max_turns, Some(12));
    }

    #[test]
    fn test_cli_parses_auth_subcommands() {
        let cli = Cli::try_parse_from(["rho", "auth", "set"]).unwrap();
        assert_eq!(
            cli.command,
            Some(Commands::Auth {
                action: Some(AuthCommands::Set)
            })
        );

        let cli = Cli::try_parse_from(["rho", "auth", "remove"]).unwrap();
        assert_eq!(
            cli.command,
            Some(Commands::Auth {
                action: Some(AuthCommands::Remove)
            })
        );
    }

    #[test]
    fn help_matches_documented_auth_sessions_limits_and_context() {
        use clap::CommandFactory;

        let mut help = Vec::new();
        Cli::command().write_long_help(&mut help).unwrap();
        let help = String::from_utf8(help).unwrap();
        for expected in [
            "openai",
            "chatgpt",
            "copilot",
            "explicit login required",
            "provider default when omitted",
            "pending budget checkpoint",
            "AI_CONTEXT_WINDOW_MESSAGES=24",
            "AI_COMPACTION_MAX_BYTES=8192",
        ] {
            assert!(help.contains(expected), "missing help text: {expected}");
        }
    }
}
