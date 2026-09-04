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

    let cli = Cli::try_parse_from(["rho", "plugin", "install", "rho-plugin-shell", "--replace", "tool:bash"]).unwrap();
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
fn help_matches_documented_auth_sessions_limits_and_context() {
    use clap::CommandFactory;

    let mut help = Vec::new();
    Cli::command().write_long_help(&mut help).unwrap();
    let help = String::from_utf8(help).unwrap();
    for expected in [
        "openai",
        "chatgpt",
        "copilot",
        "antigravity",
        "explicit login required",
        "provider default when omitted",
        "pending budget checkpoint",
        "AI_CONTEXT_WINDOW_MESSAGES",
        "AI_COMPACTION_MAX_BYTES",
    ] {
        assert!(help.contains(expected), "missing help text: {expected}");
    }
}
