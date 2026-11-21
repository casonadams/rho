use super::super::Config;
use crate::config::cli::Cli;
use crate::provider::{ModelRoute, canonical_provider};

pub(super) fn resolve_model_with_provider(config: &mut Config, model_spec: &str, provider_flag: Option<&str>) {
    let route = ModelRoute::parse(model_spec, provider_flag, "local");
    if let Some(flag) = provider_flag.filter(|p| !p.trim().is_empty())
        && canonical_provider(flag) != route.provider
    {
        let warning = format!(
            "Warning: Provider '{}' overridden by provider in model spec '{}'.",
            flag.trim(),
            route.provider
        );
        eprintln!("{warning}");
        config.migration_warnings.push(warning);
    }
    config.set_route(route);
}

fn apply_provider_flag_only(config: &mut Config, provider: &str) {
    let provider = provider.trim();
    let provider_changed = config.provider != provider;
    config.provider = provider.to_string();
    if provider_changed {
        config.model = config.models.get(provider).cloned().unwrap_or_default();
    }
}

fn apply_model_cli_overrides(config: &mut Config, c: &Cli) {
    if let Some(ref m) = c.model {
        resolve_model_with_provider(config, m.trim(), c.provider.as_deref());
    } else if c.smol {
        if let Some(smol_model) = config.models.get("smol").cloned() {
            resolve_model_with_provider(config, smol_model.trim(), c.provider.as_deref());
        }
    } else if c.slow {
        if let Some(slow_model) = config.models.get("slow").cloned() {
            resolve_model_with_provider(config, slow_model.trim(), c.provider.as_deref());
        }
    } else if let Some(ref p) = c.provider {
        apply_provider_flag_only(config, p);
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
    if !c.hook_dir.is_empty() {
        config.hook_dirs.extend(c.hook_dir.clone());
    }
}
