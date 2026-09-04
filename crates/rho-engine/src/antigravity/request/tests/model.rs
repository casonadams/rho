use super::*;
use rig::message::UserContent;

#[test]
fn runtime_mapping_covers_known_families_and_passes_through_unknown() {
    assert_eq!(
        resolve_runtime_model("gemini-3.8-flash", Effort::Off),
        "gemini-3.8-flash-low"
    );
    assert_eq!(
        resolve_runtime_model("gemini-3.5-flash", Effort::Off),
        "gemini-3.5-flash-extra-low"
    );
    assert_eq!(
        resolve_runtime_model("claude-opus-4-6", Effort::Off),
        "claude-opus-4-6-thinking"
    );
    assert_eq!(
        resolve_runtime_model("gpt-oss-120b", Effort::Off),
        "gpt-oss-120b-medium"
    );
    // Runtime ids from the live catalog pass through untouched.
    assert_eq!(
        resolve_runtime_model("gemini-3.8-flash-high", Effort::High),
        "gemini-3.8-flash-high"
    );
    assert_eq!(
        resolve_runtime_model("claude-sonnet-4-6", Effort::High),
        "claude-sonnet-4-6"
    );
}

#[test]
fn fallback_chain_degrades_next_generation() {
    assert_eq!(
        fallback_runtime_model("gemini-3.8-flash-low"),
        Some("gemini-3.7-flash-low".to_string())
    );
    assert_eq!(
        fallback_runtime_model("gemini-3.7-flash-medium"),
        Some("gemini-3.6-flash-medium".to_string())
    );
    assert_eq!(fallback_runtime_model("gemini-3.6-flash-low"), None);
    assert_eq!(fallback_runtime_model("claude-sonnet-4-6"), None);
}

#[test]
fn max_tokens_is_capped_per_runtime_family() {
    let mut request = minimal_request(vec![Message::User {
        content: vec![UserContent::text("hi")],
    }]);
    request.max_tokens = Some(1_000_000);
    let body = build_request_body(target("p", "claude-sonnet-4-6"), &request, &envelope()).unwrap();
    assert_eq!(body["request"]["generationConfig"]["maxOutputTokens"], 64000);

    let body = build_request_body(target("p", "gpt-oss-120b-medium"), &request, &envelope()).unwrap();
    assert_eq!(body["request"]["generationConfig"]["maxOutputTokens"], 32768);
}

#[test]
fn model_enum_label_uses_rollout_ids() {
    let request = minimal_request(vec![Message::User {
        content: vec![UserContent::text("hi")],
    }]);
    let body = build_request_body(target("p", "gemini-3.5-flash-extra-low"), &request, &envelope()).unwrap();
    assert_eq!(body["request"]["labels"]["model_enum"], "MODEL_PLACEHOLDER_M187");
}

#[test]
fn thinking_level_routes_runtime_variants() {
    assert_eq!(
        resolve_runtime_model("gemini-3.7-flash", Effort::Off),
        "gemini-3.7-flash-low"
    );
    assert_eq!(
        resolve_runtime_model("gemini-3.7-flash", Effort::Low),
        "gemini-3.7-flash-low"
    );
    assert_eq!(
        resolve_runtime_model("gemini-3.7-flash", Effort::Medium),
        "gemini-3.7-flash-medium"
    );
    assert_eq!(
        resolve_runtime_model("gemini-3.7-flash", Effort::High),
        "gemini-3.7-flash-high"
    );
    // xhigh/max have no finer backend level; they ride high.
    assert_eq!(Effort::parse(Some("xhigh")), Effort::High);
    assert_eq!(Effort::parse(Some("max")), Effort::High);
    assert_eq!(Effort::parse(None), Effort::Off);
    // Agent aliases are the high variant of their family.
    assert_eq!(
        resolve_runtime_model("gemini-3.1-pro", Effort::High),
        "gemini-pro-agent"
    );
    assert_eq!(
        resolve_runtime_model("gemini-3.1-pro", Effort::Medium),
        "gemini-3.1-pro-low"
    );
    assert_eq!(
        resolve_runtime_model("gemini-3.5-flash", Effort::High),
        "gemini-3-flash-agent"
    );
}

#[test]
fn collapse_runtime_id_folds_tiers_into_families() {
    let (base, level) = collapse_runtime_id("gemini-3.7-flash-high");
    assert_eq!(base, "gemini-3.7-flash");
    assert_eq!(level, Some(Effort::High));

    let (base, level) = collapse_runtime_id("gemini-3.5-flash-extra-low");
    assert_eq!(base, "gemini-3.5-flash");
    assert_eq!(level, Some(Effort::Low));

    let (base, level) = collapse_runtime_id("gemini-3.6-flash-tiered");
    assert_eq!(base, "gemini-3.6-flash");
    assert_eq!(level, None);

    let (base, level) = collapse_runtime_id("gemini-3-flash-agent");
    assert_eq!(base, "gemini-3.5-flash");
    assert_eq!(level, Some(Effort::High));

    let (base, level) = collapse_runtime_id("claude-sonnet-4-6");
    assert_eq!(base, "claude-sonnet-4-6");
    assert_eq!(level, None);

    let (base, level) = collapse_runtime_id("gpt-oss-120b-medium");
    assert_eq!(base, "gpt-oss-120b");
    assert_eq!(level, Some(Effort::Medium));
}

#[test]
fn thinking_config_tracks_effort() {
    let request = minimal_request(vec![Message::User {
        content: vec![UserContent::text("hi")],
    }]);

    // Gemini flash: thinkingLevel follows the effort.
    let body = build_request_body(high_target("p", "gemini-3.7-flash-high"), &request, &envelope()).unwrap();
    assert_eq!(
        body["request"]["generationConfig"]["thinkingConfig"]["thinkingLevel"],
        "HIGH"
    );
    assert_eq!(
        body["request"]["generationConfig"]["thinkingConfig"]["includeThoughts"],
        true
    );

    // Off: includeThoughts false.
    let body = build_request_body(target("p", "gemini-3.7-flash-low"), &request, &envelope()).unwrap();
    assert_eq!(
        body["request"]["generationConfig"]["thinkingConfig"]["includeThoughts"],
        false
    );

    // 3.1-pro high routes to the agent id and uses a thinking budget.
    let body = build_request_body(high_target("p", "gemini-pro-agent"), &request, &envelope()).unwrap();
    assert_eq!(
        body["request"]["generationConfig"]["thinkingConfig"]["thinkingBudget"],
        10001
    );

    // Claude takes no gemini thinkingConfig (beta header path instead).
    let body = build_request_body(high_target("p", "claude-sonnet-4-6"), &request, &envelope()).unwrap();
    assert!(body["request"]["generationConfig"].get("thinkingConfig").is_none());
    assert!(wants_claude_thinking_header("claude-sonnet-4-6", Effort::High));
    assert!(!wants_claude_thinking_header("claude-sonnet-4-6", Effort::Off));
    assert!(!wants_claude_thinking_header("gemini-3.7-flash-high", Effort::High));
}
