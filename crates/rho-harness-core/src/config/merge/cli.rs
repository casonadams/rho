use super::super::Config;
use crate::config::cli::Cli;

fn apply_model_cli_overrides(config: &mut Config, c: &Cli) {
    if let Some(ref m) = c.model {
        config.model = m.clone();
    }
    if let Some(ref p) = c.provider {
        config.provider = p.clone();
    }
    if let Some(ref t) = c.thinking {
        config.thinking_level = (t != "off").then(|| t.clone());
    }
}

fn apply_limit_cli_overrides(config: &mut Config, c: &Cli) {
    if let Some(max_output_tokens) = c.max_output_tokens {
        config.max_output_tokens = Some(max_output_tokens);
    }
    if let Some(max_turns) = c.max_turns {
        config.max_turns = max_turns;
    }
}

fn apply_prompt_cli_overrides(config: &mut Config, c: &Cli) {
    if let Some(ref sp) = c.system_prompt {
        config.system_prompt = Some(sp.clone());
    }
    if let Some(ref asp) = c.append_system_prompt {
        config.append_system_prompt = Some(asp.clone());
    }
    if c.no_context_files {
        config.no_context_files = true;
    }
    if c.no_permission {
        config.permission.enabled = false;
    }
}

pub(crate) fn apply_cli_overrides(config: &mut Config, cli: Option<&Cli>) {
    let Some(c) = cli else {
        return;
    };
    apply_model_cli_overrides(config, c);
    apply_limit_cli_overrides(config, c);
    apply_prompt_cli_overrides(config, c);
}
