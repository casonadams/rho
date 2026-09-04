#[cfg(test)]
mod tests {
    use crate::config::Config;
    use crate::config::cli::Cli;
    use clap::Parser;

    #[test]
    fn test_cli_flags_parsing() {
        let args = [
            "rho",
            "--thinking",
            "high",
            "-n",
            "My Session",
            "--export",
            "out.md",
            "first prompt",
            "second prompt",
        ];
        let cli = Cli::parse_from(args);
        assert_eq!(cli.thinking.as_deref(), Some("high"));
        assert_eq!(cli.name.as_deref(), Some("My Session"));
        assert_eq!(cli.export.as_deref(), Some("out.md"));
        assert_eq!(cli.message, vec!["first prompt", "second prompt"]);

        let config = Config::load(Some(&cli)).unwrap();
        assert_eq!(config.thinking_level.as_deref(), Some("high"));
    }
}
