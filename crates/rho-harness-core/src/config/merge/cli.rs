use super::super::Config;
use crate::config::cli::Cli;

pub(crate) fn apply_cli_overrides(config: &mut Config, cli: Option<&Cli>) {
    let Some(c) = cli else {
        return;
    };
    if let Some(ref m) = c.model {
        config.model = m.clone();
    }
    if let Some(ref p) = c.provider {
        config.provider = p.clone();
    }
    if let Some(max_output_tokens) = c.max_output_tokens {
        config.max_output_tokens = Some(max_output_tokens);
    }
    if let Some(max_turns) = c.max_turns {
        config.max_turns = max_turns;
    }
    if let Some(ref t) = c.thinking {
        config.thinking_level = if t == "off" { None } else { Some(t.clone()) };
    }
    if c.auto_approve {
        config.auto_approve = true;
    }
}
