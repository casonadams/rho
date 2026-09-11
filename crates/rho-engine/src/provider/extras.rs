//! Per-provider request extras carried on every agent completion via rig's
//! agent-level `additional_params`.
//!
//! OpenAI Responses-based providers (openai, chatgpt/codex) stream reasoning
//! summaries only when the request opts in with `reasoning.summary`, and route
//! prompt-cache hits through `prompt_cache_key`. The Anthropic API takes an
//! extended-thinking budget; Gemini streams thoughts only when
//! `thinkingConfig.includeThoughts` is set.

use rho_harness_core::provider::ProviderId;
use serde_json::{Value, json};

use std::str::FromStr;

/// Build the `additional_params` payload for one provider, or `None` when the
/// provider needs no extras. `thinking_level` is rho's selected level; `None`
/// or "off" means thinking is disabled.
pub fn provider_request_extras(provider: &str, thinking_level: Option<&str>, session_id: &str) -> Option<Value> {
    let provider = ProviderId::from_str(provider.trim()).ok()?;
    match provider {
        ProviderId::OpenAi | ProviderId::ChatGpt => Some(responses_extras(enabled_level(thinking_level), session_id)),
        ProviderId::Anthropic => crate::claude::request::resolve_thinking_budget(thinking_level)
            .map(|budget| json!({ "thinking": { "type": "enabled", "budget_tokens": budget } })),
        ProviderId::Gemini => enabled_level(thinking_level)
            .map(|_| json!({ "generationConfig": { "thinkingConfig": { "includeThoughts": true } } })),
        _ => None,
    }
}

fn enabled_level(level: Option<&str>) -> Option<&str> {
    level
        .map(str::trim)
        .filter(|level| !level.is_empty() && !level.eq_ignore_ascii_case("off"))
}

fn responses_extras(thinking: Option<&str>, session_id: &str) -> Value {
    let mut extras = json!({ "prompt_cache_key": session_id });
    if let Some(effort) = thinking.and_then(reasoning_effort) {
        extras["reasoning"] = json!({ "effort": effort, "summary": "auto" });
    }
    extras
}

fn reasoning_effort(level: &str) -> Option<&'static str> {
    match level.to_ascii_lowercase().as_str() {
        "minimal" => Some("minimal"),
        "low" => Some("low"),
        "medium" => Some("medium"),
        "high" => Some("high"),
        "xhigh" => Some("xhigh"),
        "max" => Some("max"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chatgpt_with_thinking_requests_effort_and_summaries() {
        let extras = provider_request_extras("chatgpt", Some("high"), "sess-1").unwrap();
        assert_eq!(
            extras,
            json!({
                "prompt_cache_key": "sess-1",
                "reasoning": { "effort": "high", "summary": "auto" },
            })
        );
    }

    #[test]
    fn chatgpt_without_thinking_keeps_only_cache_key() {
        for level in [None, Some("off"), Some("  ")] {
            let extras = provider_request_extras("chatgpt", level, "sess-1").unwrap();
            assert_eq!(extras, json!({ "prompt_cache_key": "sess-1" }));
        }
    }

    #[test]
    fn openai_passes_supported_efforts_through() {
        let cases = [
            ("minimal", "minimal"),
            ("low", "low"),
            ("medium", "medium"),
            ("high", "high"),
            ("xhigh", "xhigh"),
            ("max", "max"),
        ];
        for (level, effort) in cases {
            let extras = provider_request_extras("openai", Some(level), "s").unwrap();
            assert_eq!(extras["reasoning"]["effort"], effort);
            assert_eq!(extras["reasoning"]["summary"], "auto");
        }
    }

    #[test]
    fn openai_ignores_unknown_levels() {
        let extras = provider_request_extras("openai", Some("turbo"), "s").unwrap();
        assert!(extras.get("reasoning").is_none());
    }

    #[test]
    fn anthropic_maps_levels_to_thinking_budget() {
        assert_eq!(
            provider_request_extras("anthropic", Some("medium"), "s").unwrap(),
            json!({ "thinking": { "type": "enabled", "budget_tokens": 4096 } })
        );
        assert_eq!(
            provider_request_extras("anthropic", Some("max"), "s").unwrap(),
            json!({ "thinking": { "type": "enabled", "budget_tokens": 16384 } })
        );
        assert!(provider_request_extras("anthropic", None, "s").is_none());
        assert!(provider_request_extras("anthropic", Some("off"), "s").is_none());
    }

    #[test]
    fn gemini_opts_into_thought_display_when_enabled() {
        assert_eq!(
            provider_request_extras("gemini", Some("low"), "s").unwrap(),
            json!({ "generationConfig": { "thinkingConfig": { "includeThoughts": true } } })
        );
        for level in [None, Some("off")] {
            assert!(provider_request_extras("gemini", level, "s").is_none());
        }
    }

    #[test]
    fn providers_without_extras_return_none() {
        for provider in [
            "claude",
            "antigravity",
            "deepseek",
            "copilot",
            "groq",
            "local",
            "my-custom",
        ] {
            assert!(
                provider_request_extras(provider, Some("high"), "s").is_none(),
                "{provider}"
            );
        }
    }
}
