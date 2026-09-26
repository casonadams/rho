use super::super::Config;
use super::super::types::{FileConfig, WebFetchConfigFile, WebSearchConfigFile};
use std::str::FromStr;

fn merge_provider_fallback(config: &mut Config, model_specified: bool) {
    if !model_specified {
        return;
    }
    if let Some((p, _)) = config.model.split_once('/') {
        let p = p.trim();
        if !p.is_empty() {
            config.provider = p.to_string();
            config.default_provider = Some(p.to_string());
            return;
        }
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

fn merge_web_search_settings(config: &mut Config, search: &WebSearchConfigFile) {
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

fn merge_web_fetch_settings(config: &mut Config, fetch: &WebFetchConfigFile) {
    if let Some(enabled) = fetch.enabled {
        config.tools.web.fetch.enabled = enabled;
    }
    if let Some(multimodal) = fetch.multimodal {
        config.tools.web.fetch.multimodal = multimodal;
    }
}

fn merge_tools_settings(config: &mut Config, file: &FileConfig) {
    let Some(web) = file.tools.as_ref().and_then(|t| t.web.as_ref()) else {
        return;
    };
    if let Some(ref search) = web.search {
        merge_web_search_settings(config, search);
    }
    if let Some(ref fetch) = web.fetch {
        merge_web_fetch_settings(config, fetch);
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

const LEGACY_MODEL_DEPRECATION_WARNING: &str = "Warning: 'model_provider' and 'providers.*.default_model' are deprecated. Use 'model = \"<provider>/<model>\"' instead.";

fn detect_legacy_model_provider(file: &mut FileConfig) -> bool {
    let Some(ref mp) = file.model_provider else {
        return false;
    };
    let provider = mp.trim();
    if let Some(ref m) = file.model {
        if !m.contains('/') {
            file.model = Some(format!("{provider}/{}", m.trim()));
        }
    } else {
        let default_m = file
            .models
            .get(provider)
            .cloned()
            .unwrap_or_else(|| crate::provider::default_model_for_provider(provider).to_string());
        file.model = Some(format!("{provider}/{default_m}"));
    }
    file.provider = Some(provider.to_string());
    true
}

fn detect_legacy_provider_default_models(file: &mut FileConfig) -> bool {
    let mut detected = false;
    let mut default_spec = None;

    for (name, provider_cfg) in &mut file.providers {
        if let Some(dm) = provider_cfg.default_model.take() {
            detected = true;
            if default_spec.is_none() {
                default_spec = Some((name.clone(), dm));
            }
        }
    }

    if (file.model.is_none() || file.model.as_deref().unwrap_or("").trim().is_empty())
        && let Some((p, dm)) = default_spec
    {
        file.model = Some(format!("{p}/{}", dm.trim()));
        file.provider = Some(p);
    }

    file.providers
        .retain(|name, p| !(p.base_url.trim().is_empty() && crate::provider::ProviderId::from_str(name).is_ok()));

    detected
}

fn migrate_legacy_model_config(config: &mut Config, file: &mut FileConfig) {
    let p1 = detect_legacy_model_provider(file);
    let p2 = detect_legacy_provider_default_models(file);
    if p1 || p2 {
        eprintln!("{LEGACY_MODEL_DEPRECATION_WARNING}");
        config
            .migration_warnings
            .push(LEGACY_MODEL_DEPRECATION_WARNING.to_string());
    }
}

pub(crate) fn merge_file(config: &mut Config, mut file: FileConfig) {
    migrate_legacy_model_config(config, &mut file);
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
    merge_tools_settings(config, &file);
}
