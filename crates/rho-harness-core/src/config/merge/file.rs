use super::super::Config;
use super::super::types::FileConfig;

pub(crate) fn merge_file(config: &mut Config, file: FileConfig) {
    if let Some(m) = file.model {
        config.model = m;
    }
    if let Some(p) = file.provider {
        config.provider = p;
    }
    if let Some(a) = file.auto_approve {
        config.auto_approve = a;
    }
    if let Some(max_output_tokens) = file.max_output_tokens {
        config.max_output_tokens = Some(max_output_tokens);
    }
    if let Some(max_turns) = file.max_turns {
        config.max_turns = max_turns;
    }
    if let Some(c) = file.context_limit {
        config.context_limit = Some(c);
    }
    if let Some(value) = file.context_window_messages {
        config.context_window_messages = value;
    }
    if let Some(value) = file.compaction_max_bytes {
        config.compaction_max_bytes = value;
    }
    if let Some(value) = file.reserve_tokens {
        config.reserve_tokens = value;
    }
    if let Some(value) = file.keep_recent_tokens {
        config.keep_recent_tokens = value;
    }
    if let Some(s) = file.search_min_interval_ms {
        config.search_min_interval_ms = s;
    }
    if let Some(s) = file.search_timeout_sec {
        config.search_timeout_sec = s;
    }
    if let Some(f) = file.fetch_timeout_sec {
        config.fetch_timeout_sec = f;
    }
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
    if let Some(r) = file.region {
        config.region = r;
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
    if let Some(t) = file.thinking_level {
        config.thinking_level = Some(t);
    }
    if let Some(tokens) = file.context_injection_max_tokens {
        config.context_injection_max_tokens = tokens;
    }
    if let Some(mcp) = file.mcp {
        config.mcp = mcp;
    }
    config.plugins = file.plugins;
    config.providers = file.providers;
}
