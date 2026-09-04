//! Antigravity catalog parsing, collapsing, recency sorting, and display names.

use super::DiscoveredModel;
use super::presets::format_context_desc;
use std::collections::BTreeMap;

/// Fold tiered runtime ids into one selectable family entry per model
/// (`gemini-3.7-flash-{low,medium,high}` → `gemini-3.7-flash`); the thinking
/// level then picks the variant at request time.
pub fn collapse_antigravity_catalog(runtime_ids: Vec<String>) -> Vec<DiscoveredModel> {
    let mut families: BTreeMap<String, Vec<Option<crate::antigravity::Effort>>> = BTreeMap::new();
    for id in &runtime_ids {
        let (base, level) = crate::antigravity::collapse_runtime_id(id);
        families.entry(base).or_default().push(level);
    }

    let models: Vec<DiscoveredModel> = families
        .into_iter()
        .map(|(base, levels)| {
            let thinking = if levels.iter().any(|l| l.is_some()) {
                " · adaptive thinking"
            } else {
                ""
            };
            DiscoveredModel {
                context_tokens: None,
                name: antigravity_display_name(&base),
                description: format!("{}{}", format_context_desc(&base), thinking),
                id: base,
                provider: "antigravity".to_string(),
            }
        })
        .collect();

    sort_models_newest_first(models)
}

/// Version-aware recency key: the first version number in the id, descending.
/// Handles dotted (`3.8`) and split-digit (`claude-4-6`) spellings.
pub fn model_recency_key(id: &str) -> (u32, u32) {
    let tokens: Vec<&str> = id.split('-').collect();
    for (index, token) in tokens.iter().enumerate() {
        if let Some((major, minor)) = token.split_once('.')
            && let (Ok(a), Ok(b)) = (major.parse::<u32>(), minor.parse::<u32>())
        {
            return (a, b);
        }
        if let Ok(a) = token.parse::<u32>()
            && let Some(Ok(b)) = tokens.get(index + 1).map(|t| t.parse::<u32>())
        {
            return (a, b);
        }
        if let Ok(a) = token.parse::<u32>() {
            return (a, 0);
        }
    }
    (0, 0)
}

/// Newest (highest version number) first; ties keep input order.
pub fn sort_models_newest_first(mut models: Vec<DiscoveredModel>) -> Vec<DiscoveredModel> {
    models.sort_by_key(|model| std::cmp::Reverse(model_recency_key(&model.id)));
    models
}

pub fn antigravity_display_name(id: &str) -> String {
    const TITLES: [(&str, &str); 11] = [
        ("gemini", "Gemini"),
        ("claude", "Claude"),
        ("gpt", "GPT"),
        ("oss", "OSS"),
        ("opus", "Opus"),
        ("sonnet", "Sonnet"),
        ("pro", "Pro"),
        ("flash", "Flash"),
        ("lite", "Lite"),
        ("thinking", "Thinking"),
        ("agent", "Agent"),
    ];
    let tokens: Vec<&str> = id.split('-').collect();
    let mut words: Vec<String> = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        let token = tokens[index];
        if let (Ok(a), Some(Ok(b))) = (token.parse::<u8>(), tokens.get(index + 1).map(|t| t.parse::<u8>())) {
            words.push(format!("{a}.{b}"));
            index += 2;
            continue;
        }
        let title = TITLES
            .iter()
            .find(|(key, _)| *key == token)
            .map(|(_, title)| (*title).to_string())
            .unwrap_or_else(|| token.to_string());
        words.push(title);
        index += 1;
    }
    words.join(" ")
}
