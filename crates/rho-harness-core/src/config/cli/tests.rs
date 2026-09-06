use super::*;

#[test]
fn test_cli_parsing_prompt() {
    let args = vec!["rho", "-p", "fix bug in auth"];
    let cli = Cli::try_parse_from(args).unwrap();
    assert_eq!(cli.prompt.as_deref(), Some("fix bug in auth"));
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
fn test_cli_parsing_plugin_list() {
    for cmd in [["rho", "plugin", "list"], ["rho", "plugin", "ls"]] {
        let cli = Cli::try_parse_from(cmd).unwrap();
        assert_eq!(
            cli.command,
            Some(Commands::Plugin {
                action: Some(PluginCommands::List)
            })
        );
    }
}

#[test]
fn test_cli_parsing_plugin_install() {
    let cli = Cli::try_parse_from(["rho", "plugin", "install", "rho-plugin-git"]).unwrap();
    assert_eq!(
        cli.command,
        Some(Commands::Plugin {
            action: Some(PluginCommands::Install {
                target: "rho-plugin-git".to_string(),
                force: false
            })
        })
    );

    for flag in ["--replace", "--force"] {
        let cli = Cli::try_parse_from(["rho", "plugin", "install", "rho-plugin-shell", flag]).unwrap();
        assert_eq!(
            cli.command,
            Some(Commands::Plugin {
                action: Some(PluginCommands::Install {
                    target: "rho-plugin-shell".to_string(),
                    force: true
                })
            })
        );
    }
}

#[test]
fn test_cli_parsing_plugin_update() {
    let cli = Cli::try_parse_from(["rho", "plugin", "update"]).unwrap();
    assert_eq!(
        cli.command,
        Some(Commands::Plugin {
            action: Some(PluginCommands::Update { target: None })
        })
    );

    let cli = Cli::try_parse_from(["rho", "plugin", "update", "git"]).unwrap();
    assert_eq!(
        cli.command,
        Some(Commands::Plugin {
            action: Some(PluginCommands::Update {
                target: Some("git".to_string())
            })
        })
    );
}

#[test]
fn test_cli_parsing_plugin_remove() {
    let cli = Cli::try_parse_from(["rho", "plugin", "remove", "git"]).unwrap();
    assert_eq!(
        cli.command,
        Some(Commands::Plugin {
            action: Some(PluginCommands::Remove {
                name: "git".to_string(),
                keep_binary: false
            })
        })
    );

    let cli = Cli::try_parse_from(["rho", "plugin", "rm", "git", "--keep-binary"]).unwrap();
    assert_eq!(
        cli.command,
        Some(Commands::Plugin {
            action: Some(PluginCommands::Remove {
                name: "git".to_string(),
                keep_binary: true
            })
        })
    );
}

#[test]
fn test_cli_parsing_plugin_inspect() {
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
fn test_cli_parsing_top_level_install() {
    let cli = Cli::try_parse_from(["rho", "install", "rho-plugin-git"]).unwrap();
    assert_eq!(
        cli.command,
        Some(Commands::Install {
            target: "rho-plugin-git".to_string(),
            force: false
        })
    );

    for flag in ["--force", "--replace"] {
        let cli = Cli::try_parse_from(["rho", "install", "rho-plugin-git", flag]).unwrap();
        assert_eq!(
            cli.command,
            Some(Commands::Install {
                target: "rho-plugin-git".to_string(),
                force: true
            })
        );
    }
}

#[test]
fn test_cli_parsing_top_level_update() {
    let cli = Cli::try_parse_from(["rho", "update"]).unwrap();
    assert_eq!(cli.command, Some(Commands::Update { target: None }));

    for target in ["all", "permission"] {
        let cli = Cli::try_parse_from(["rho", "update", target]).unwrap();
        assert_eq!(
            cli.command,
            Some(Commands::Update {
                target: Some(target.to_string())
            })
        );
    }
}

#[test]
fn test_cli_parsing_top_level_remove() {
    for cmd in [
        ["rho", "remove", "git"],
        ["rho", "uninstall", "git"],
        ["rho", "rm", "git"],
    ] {
        let cli = Cli::try_parse_from(cmd).unwrap();
        assert_eq!(
            cli.command,
            Some(Commands::Remove {
                name: "git".to_string(),
                keep_binary: false
            })
        );
    }

    let cli = Cli::try_parse_from(["rho", "remove", "git", "--keep-binary"]).unwrap();
    assert_eq!(
        cli.command,
        Some(Commands::Remove {
            name: "git".to_string(),
            keep_binary: true
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
fn test_cli_flags() {
    let cli = Cli::try_parse_from([
        "rho",
        "--system-prompt",
        "custom system prompt",
        "--append-system-prompt",
        "append instructions",
        "--no-context-files",
        "--no-permission",
    ])
    .unwrap();
    let actual = (
        cli.system_prompt.as_deref(),
        cli.append_system_prompt.as_deref(),
        cli.no_context_files,
        cli.no_permission,
    );
    assert_eq!(
        actual,
        (Some("custom system prompt"), Some("append instructions"), true, true)
    );
}

#[test]
fn test_cli_flag_aliases() {
    let cli = Cli::try_parse_from(["rho", "--nc"]).unwrap();
    assert_eq!((cli.no_context_files, cli.no_permission), (true, false));
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
        "claude",
        "explicit login required",
        "provider default when omitted",
        "pending budget checkpoint",
        "AI_CONTEXT_WINDOW_MESSAGES",
        "AI_COMPACTION_MAX_BYTES",
    ] {
        assert!(help.contains(expected), "missing help text: {expected}");
    }
}
