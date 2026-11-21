//! Antigravity's fallback catalog (it has no public model-list API) and context descriptions.

use super::DiscoveredModel;

pub(crate) fn format_context_desc(model_id: &str) -> String {
    format_context_tokens(rho_harness_core::tokens::context_window_size(model_id))
}

pub fn format_context_tokens(ctx: usize) -> String {
    if ctx.is_power_of_two() {
        if ctx >= 1024 * 1024 {
            format!("{}M ctx", ctx / (1024 * 1024))
        } else {
            format!("{}k ctx", ctx / 1024)
        }
    } else if ctx >= 1_000_000 {
        format!("{}M ctx", ctx / 1_000_000)
    } else {
        format!("{}k ctx", ctx / 1000)
    }
}

fn build_presets(table: &[(&str, &str, &str, &str)]) -> Vec<DiscoveredModel> {
    table
        .iter()
        .map(|&(id, name, provider, desc)| DiscoveredModel::new(id, name, provider, desc))
        .collect()
}

const ANTIGRAVITY_PRESETS: &[(&str, &str, &str, &str)] = &[
    (
        "gemini-4-carbon",
        "Gemini 4 Carbon",
        "antigravity",
        "1M ctx · deepest reasoning",
    ),
    (
        "gemini-4-neon",
        "Gemini 4 Neon",
        "antigravity",
        "1M ctx · frontier coding",
    ),
    (
        "gemini-4-argon",
        "Gemini 4 Argon",
        "antigravity",
        "1M ctx · frontier reasoning",
    ),
    ("gemini-4-hydrogen", "Gemini 4 Hydrogen", "antigravity", "1M ctx · fast"),
    ("gemini-3.8-flash", "Gemini 3.8 Flash", "antigravity", "1M ctx · fast"),
    ("gemini-3.7-flash", "Gemini 3.7 Flash", "antigravity", "1M ctx · fast"),
    ("gemini-3.1-pro", "Gemini 3.1 Pro", "antigravity", "1M ctx · reasoning"),
    (
        "claude-sonnet-4-6",
        "Claude Sonnet 4.6",
        "antigravity",
        "200k ctx · reasoning",
    ),
    (
        "claude-opus-4-6",
        "Claude Opus 4.6",
        "antigravity",
        "250k ctx · deep reasoning",
    ),
    ("gpt-oss-120b", "GPT-OSS 120B", "antigravity", "128k ctx · open"),
];

pub fn antigravity_preset_models() -> Vec<DiscoveredModel> {
    super::antigravity::sort_models_newest_first(build_presets(ANTIGRAVITY_PRESETS))
}
