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
        if !model_specified {
            if let Some(configured) = config.models.get(p) {
                config.model = configured.clone();
                config.default_model = Some(configured.clone());
            } else {
                let default_m = crate::provider::default_model_for_provider(p);
                config.model = default_m.to_string();
                config.default_model = Some(default_m.to_string());
            }
        }
    } else {
        if !model_specified && let Some(configured) = config.models.get(&config.provider) {
            config.model = configured.clone();
        }
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

fn merge_permission_and_providers(config: &mut Config, file: FileConfig) {
    if let Some(permission) = file.permission {
        config.permission = permission;
    }
    config.providers = file.providers;
}

fn merge_ui_settings(config: &mut Config, file: &FileConfig) {
    if let Some(ref ui) = file.ui {
        config.ui.merge(ui);
    }
}

fn merge_features(config: &mut Config, file: &FileConfig) {
    if let Some(s) = file.semantic_search {
        config.semantic_search = s;
    } else if let Some(ref features) = file.features
        && let Some(s) = features.semantic_search
    {
        config.semantic_search = s;
    }
}

fn merge_tools_settings(config: &mut Config, file: &FileConfig) {
    if let Some(ref tools) = file.tools
        && let Some(ref web) = tools.web
    {
        if let Some(ref search) = web.search {
            if let Some(enabled) = search.enabled {
                config.tools.web.search.enabled = enabled;
            }
            if let Some(ref default) = search.default {
                config.tools.web.search.default = default.clone();
            }
            if let Some(ref fallback) = search.fallback {
                config.tools.web.search.fallback = fallback.clone();
            }
        }
        if let Some(fetch) = &web.fetch {
            if let Some(enabled) = fetch.enabled {
                config.tools.web.fetch.enabled = enabled;
            }
            if let Some(multimodal) = fetch.multimodal {
                config.tools.web.fetch.multimodal = multimodal;
            }
        }
    }
}

fn merge_mcp_settings(config: &mut Config, file: &FileConfig) {
    if let Some(ref mcp) = file.mcp {
        if let Some(enabled) = mcp.enabled {
            config.mcp.enabled = enabled;
        }
        if let Some(threshold) = mcp.defer_threshold {
            config.mcp.defer_threshold = threshold;
        }
        config.mcp.servers.extend(mcp.servers.clone());
    }
}

pub(crate) fn merge_file(config: &mut Config, file: FileConfig) {
    config.models.extend(file.models.clone());
    merge_model_and_provider(config, &file);
    merge_token_limits(config, &file);
    merge_context_settings(config, &file);
    merge_reserve_settings(config, &file);
    merge_search_settings(config, &file);
    merge_fetch_settings(config, &file);
    merge_modes(config, &file);
    merge_retention(config, &file);
    merge_permission_and_providers(config, file.clone());
    merge_mcp_settings(config, &file);
    merge_ui_settings(config, &file);
    merge_features(config, &file);
    merge_tools_settings(config, &file);
}
