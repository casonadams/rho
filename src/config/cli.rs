use clap::{Parser, Subcommand};

#[derive(Parser, Debug, Clone)]
#[command(name = "rust-ai", author, version, about = "Minimal agentic coding CLI harness")]
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

    /// Automatically approve tool execution without asking
    #[arg(short = 'y', long = "auto-approve", default_value_t = false)]
    pub auto_approve: bool,

    /// Resume a previous session by ID
    #[arg(long = "resume")]
    pub resume: Option<String>,

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
    /// Display or edit configuration
    Config {
        /// Config key to inspect or set
        key: Option<String>,
        /// New value for key
        value: Option<String>,
    },
    /// List live provider models when supported, otherwise curated examples
    Models,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_parsing_prompt() {
        let args = vec!["rust-ai", "-p", "fix bug in auth", "-y"];
        let cli = Cli::try_parse_from(args).unwrap();
        assert_eq!(cli.prompt.as_deref(), Some("fix bug in auth"));
        assert!(cli.auto_approve);
    }

    #[test]
    fn test_cli_parsing_subcommand() {
        let args = vec!["rust-ai", "login", "anthropic"];
        let cli = Cli::try_parse_from(args).unwrap();
        assert_eq!(
            cli.command,
            Some(Commands::Login {
                provider: Some("anthropic".to_string())
            })
        );
    }

    #[test]
    fn test_cli_parses_runtime_limits() {
        let cli = Cli::try_parse_from(["rust-ai", "--max-output-tokens", "8192", "--max-turns", "12"]).unwrap();
        assert_eq!(cli.max_output_tokens, Some(8192));
        assert_eq!(cli.max_turns, Some(12));
    }
}
