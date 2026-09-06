use super::*;
use rig::message::UserContent;

#[test]
fn runtime_mapping_covers_known_families_and_passes_through_unknown() {
    let cases = [
        ("gemini-3.8-flash", Effort::Off, "gemini-3.8-flash-low"),
        ("gemini-3.5-flash", Effort::Off, "gemini-3.5-flash-extra-low"),
        ("claude-opus-4-6", Effort::Off, "claude-opus-4-6-thinking"),
        ("gpt-oss-120b", Effort::Off, "gpt-oss-120b-medium"),
        ("gemini-3.8-flash-high", Effort::High, "gemini-3.8-flash-high"),
        ("claude-sonnet-4-6", Effort::High, "claude-sonnet-4-6"),
    ];
    for (model, effort, expected) in cases {
        assert_eq!(resolve_runtime_model(model, effort), expected);
    }
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
    let cases = [
        ("gemini-3.7-flash", Effort::Off, "gemini-3.7-flash-low"),
        ("gemini-3.7-flash", Effort::Low, "gemini-3.7-flash-low"),
        ("gemini-3.7-flash", Effort::Medium, "gemini-3.7-flash-medium"),
        ("gemini-3.7-flash", Effort::High, "gemini-3.7-flash-high"),
        ("gemini-3.1-pro", Effort::High, "gemini-pro-agent"),
        ("gemini-3.1-pro", Effort::Medium, "gemini-3.1-pro-low"),
        ("gemini-3.5-flash", Effort::High, "gemini-3-flash-agent"),
    ];
    for (model, effort, expected) in cases {
        assert_eq!(resolve_runtime_model(model, effort), expected);
    }
}

#[test]
fn effort_parse_max_and_off() {
    assert_eq!(Effort::parse(Some("xhigh")), Effort::High);
    assert_eq!(Effort::parse(Some("max")), Effort::High);
    assert_eq!(Effort::parse(None), Effort::Off);
}

#[test]
fn collapse_runtime_id_folds_tiers_into_families() {
    let cases = [
        ("gemini-3.7-flash-high", "gemini-3.7-flash", Some(Effort::High)),
        ("gemini-3.5-flash-extra-low", "gemini-3.5-flash", Some(Effort::Low)),
        ("gemini-3.6-flash-tiered", "gemini-3.6-flash", None),
        ("gemini-3-flash-agent", "gemini-3.5-flash", Some(Effort::High)),
        ("claude-sonnet-4-6", "claude-sonnet-4-6", None),
        ("gpt-oss-120b-medium", "gpt-oss-120b", Some(Effort::Medium)),
    ];
    for (id, exp_base, exp_level) in cases {
        let (base, level) = collapse_runtime_id(id);
        assert_eq!((base.as_str(), level), (exp_base, exp_level));
    }
}

#[test]
fn thinking_config_tracks_gemini() {
    let request = minimal_request(vec![Message::User {
        content: vec![UserContent::text("hi")],
    }]);
    let body = build_request_body(high_target("p", "gemini-3.7-flash-high"), &request, &envelope()).unwrap();
    let cfg = &body["request"]["generationConfig"]["thinkingConfig"];
    assert_eq!(
        (cfg["thinkingLevel"].as_str(), cfg["includeThoughts"].as_bool()),
        (Some("HIGH"), Some(true))
    );

    let body_off = build_request_body(target("p", "gemini-3.7-flash-low"), &request, &envelope()).unwrap();
    assert_eq!(
        body_off["request"]["generationConfig"]["thinkingConfig"]["includeThoughts"],
        false
    );

    let body_pro = build_request_body(high_target("p", "gemini-pro-agent"), &request, &envelope()).unwrap();
    assert_eq!(
        body_pro["request"]["generationConfig"]["thinkingConfig"]["thinkingBudget"],
        10001
    );
}

#[test]
fn thinking_config_tracks_claude_headers() {
    let request = minimal_request(vec![Message::User {
        content: vec![UserContent::text("hi")],
    }]);
    let body = build_request_body(high_target("p", "claude-sonnet-4-6"), &request, &envelope()).unwrap();
    assert!(body["request"]["generationConfig"].get("thinkingConfig").is_none());
    assert!(wants_claude_thinking_header("claude-sonnet-4-6", Effort::High));
    assert!(!wants_claude_thinking_header("claude-sonnet-4-6", Effort::Off));
    assert!(!wants_claude_thinking_header("gemini-3.7-flash-high", Effort::High));
}
