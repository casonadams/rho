use super::super::Config;
use crate::config::cli::Cli;

fn resolve_model_with_provider(config: &mut Config, model_spec: &str, provider_flag: Option<&str>) {
    let (p, _) = crate::provider::parse_model_spec(model_spec);
    if !p.is_empty() {
        if let Some(prov) = provider_flag {
            let prov = prov.trim();
            if !prov.is_empty() && prov != p {
                let warning = format!("Warning: Provider '{prov}' overridden by provider in model spec '{p}'.");
                eprintln!("{warning}");
                config.migration_warnings.push(warning);
            }
        }
        config.provider = p;
    } else if let Some(prov) = provider_flag {
        config.provider = prov.trim().to_string();
    } else if let Some(inferred) = crate::provider::infer_provider_for_model(model_spec) {
        config.provider = inferred.to_string();
    } else {
        config.provider = "local".to_string();
    }
    config.model = model_spec.to_string();
}

fn apply_provider_flag_only(config: &mut Config, provider: &str) {
    let provider = provider.trim();
    let provider_changed = config.provider != provider;
    config.provider = provider.to_string();
    if provider_changed {
        if let Some(configured) = config.models.get(provider) {
            config.model = configured.clone();
        } else {
            config.model = crate::provider::default_model_for_provider(provider).to_string();
        }
    }
}

fn apply_model_cli_overrides(config: &mut Config, c: &Cli) {
    if let Some(ref m) = c.model {
        resolve_model_with_provider(config, m.trim(), c.provider.as_deref());
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
}
