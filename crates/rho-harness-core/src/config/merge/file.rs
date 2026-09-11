use super::super::Config;
use super::super::types::FileConfig;

fn merge_provider_fallback(config: &mut Config, model_specified: bool) {
    if !model_specified {
        return;
    }
    if let Some(inferred) = crate::provider::infer_provider_for_model(&config.model) {
        config.provider = inferred.to_string();
        config.default_provider = Some(inferred.to_string());
    } else {
        config.provider = "local".to_string();
        config.default_provider = None;
    }
}

fn merge_model_and_provider(config: &mut Config, file: &FileConfig) {
    let model_specified = file.model.is_some();
    if let Some(ref m) = file.model {
        config.model = m.clone();
        config.default_model = Some(m.clone());
    }
    if let Some(ref p) = file.provider {
        config.provider = p.clone();
        config.default_provider = Some(p.clone());
    } else {
        merge_provider_fallback(config, model_specified);
    }
    if let Some(ref t) = file.thinking_level {
        config.thinking_level = (t != "off").then(|| t.clone());
    }
}

fn merge_token_limits(config: &mut Config, file: &FileConfig) {
    if let Some(max_output_tokens) = file.max_output_tokens {
        config.max_output_tokens = Some(max_output_tokens);
    }
    if let Some(max_turns) = file.max_turns {
        config.max_turns = max_turns;
    }
    if let Some(c) = file.context_limit {
        config.context_limit = Some(c);
    }
}

fn merge_context_settings(config: &mut Config, file: &FileConfig) {
    if let Some(v) = file.context_window_messages {
        config.context_window_messages = v;
    }
    if let Some(v) = file.compaction_max_bytes {
        config.compaction_max_bytes = v;
    }
    if let Some(tokens) = file.context_injection_max_tokens {
        config.context_injection_max_tokens = tokens;
    }
}

fn merge_reserve_settings(config: &mut Config, file: &FileConfig) {
    if let Some(v) = file.reserve_tokens {
        config.reserve_tokens = v;
    }
    if let Some(v) = file.keep_recent_tokens {
        config.keep_recent_tokens = v;
    }
}

fn merge_search_settings(config: &mut Config, file: &FileConfig) {
    if let Some(s) = file.search_min_interval_ms {
        config.search_min_interval_ms = s;
    }
    if let Some(s) = file.search_timeout_sec {
        config.search_timeout_sec = s;
    }
    if let Some(f) = file.fetch_timeout_sec {
        config.fetch_timeout_sec = f;
    }
}

fn merge_fetch_settings(config: &mut Config, file: &FileConfig) {
    if let Some(l) = file.fetch_limit {
        config.fetch_limit = l;
    }
    if let Some(b) = file.fetch_max_bytes {
        config.fetch_max_bytes = b;
    }
    if let Some(o) = file.output_max_bytes {
        config.output_max_bytes = o;
    }
    if let Some(p) = file.allow_private_network {
        config.allow_private_network = p;
    }
}

fn merge_modes(config: &mut Config, file: &FileConfig) {
    if let Some(ref r) = file.region {
        config.region = r.clone();
    }
    if let Some(v) = file.show_label {
        config.show_label = v;
    }
    if let Some(s) = file.steering_mode {
        config.steering_mode = s;
    }
    if let Some(f) = file.follow_up_mode {
        config.follow_up_mode = f;
    }
}

fn merge_retention(config: &mut Config, file: &FileConfig) {
    if let Some(days) = file.session_retention_days {
        config.session_retention_days = (days != 0).then_some(days);
    }
}

fn merge_plugins_and_extensions(config: &mut Config, file: FileConfig) {
    if let Some(mcp) = file.mcp {
        config.mcp.enabled = mcp.enabled;
        config.mcp.servers.extend(mcp.servers);
    }
    if let Some(permission) = file.permission {
        config.permission = permission;
    }
    config.plugins = file.plugins;
    config.providers = file.providers;
}

pub(crate) fn merge_file(config: &mut Config, file: FileConfig) {
    merge_model_and_provider(config, &file);
    merge_token_limits(config, &file);
    merge_context_settings(config, &file);
    merge_reserve_settings(config, &file);
    merge_search_settings(config, &file);
    merge_fetch_settings(config, &file);
    merge_modes(config, &file);
    merge_retention(config, &file);
    merge_plugins_and_extensions(config, file);
}
