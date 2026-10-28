use super::*;
use rho_harness_core::model::ChatMessage;

fn sample_user_request() -> ModelCompletionRequest {
    minimal_request(vec![ChatMessage::user("hi")])
}

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
    let cases = [
        ("gemini-4-argon", Some("gemini-3.8-flash-medium")),
        ("gemini-3.8-flash-low", Some("gemini-3.7-flash-low")),
        ("gemini-3.7-flash-medium", Some("gemini-3.6-flash-medium")),
        ("gemini-3.6-flash-low", None),
        ("claude-sonnet-4-6", None),
    ];
    for (runtime, expected) in cases {
        assert_eq!(fallback_runtime_model(runtime).as_deref(), expected);
    }
}

#[test]
fn max_tokens_is_capped_per_runtime_family() {
    let mut request = sample_user_request();
    request.max_tokens = Some(1_000_000);
    let body = build_request_body(target("p", "claude-sonnet-4-6"), &request, &envelope()).unwrap();
    assert_eq!(body["request"]["generationConfig"]["maxOutputTokens"], 64000);

    let body = build_request_body(target("p", "gpt-oss-120b-medium"), &request, &envelope()).unwrap();
    assert_eq!(body["request"]["generationConfig"]["maxOutputTokens"], 32768);
}

#[test]
fn model_enum_label_uses_rollout_ids() {
    let request = sample_user_request();
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
    assert_eq!(Effort::parse(Some("high")), Effort::High);
    assert_eq!(Effort::parse(Some("medium")), Effort::Medium);
    assert_eq!(Effort::parse(Some("low")), Effort::Low);
    assert_eq!(Effort::parse(Some("off")), Effort::Off);
    assert_eq!(Effort::parse(None), Effort::Off);
}

#[test]
fn collapse_runtime_id_preserves_canonical() {
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
    let request = sample_user_request();
    let body = build_request_body(high_target("p", "gemini-3.7-flash-high"), &request, &envelope()).unwrap();
    let cfg = &body["request"]["generationConfig"]["thinkingConfig"];
    assert_eq!(
        (cfg["thinkingBudget"].as_i64(), cfg["includeThoughts"].as_bool()),
        (Some(-1), Some(true))
    );

    let body_off = build_request_body(target("p", "gemini-3.7-flash-low"), &request, &envelope()).unwrap();
    assert_eq!(
        (
            body_off["request"]["generationConfig"]["thinkingConfig"]["includeThoughts"].as_bool(),
            body_off["request"]["generationConfig"]["thinkingConfig"]["thinkingBudget"].as_i64(),
        ),
        (Some(false), Some(0))
    );

    let body_pro = build_request_body(high_target("p", "gemini-pro-agent"), &request, &envelope()).unwrap();
    assert_eq!(
        body_pro["request"]["generationConfig"]["thinkingConfig"]["thinkingBudget"],
        10001
    );

    let body_medium = build_request_body(
        RequestTarget {
            project: "p",
            runtime_model: "gemini-3.8-flash-medium",
            effort: Effort::Medium,
        },
        &request,
        &envelope(),
    )
    .unwrap();
    let med_cfg = &body_medium["request"]["generationConfig"]["thinkingConfig"];
    assert_eq!(
        (med_cfg["thinkingBudget"].as_i64(), med_cfg["includeThoughts"].as_bool()),
        (Some(4000), Some(true))
    );
}

#[test]
fn thinking_config_tracks_claude_headers() {
    let request = sample_user_request();
    let body = build_request_body(high_target("p", "claude-sonnet-4-6"), &request, &envelope()).unwrap();
    assert!(body["request"]["generationConfig"].get("thinkingConfig").is_none());
    assert!(wants_claude_thinking_header("claude-sonnet-4-6", Effort::High));
    assert!(!wants_claude_thinking_header("claude-sonnet-4-6", Effort::Off));
    assert!(!wants_claude_thinking_header("gemini-3.7-flash-high", Effort::High));
}

#[test]
fn gemini_4_argon_thinking_config_and_envelope() {
    assert!(is_gemini_4("gemini-4-argon"));
    assert!(is_gemini_4("gemini-4-argon-cyber-permissive"));
    assert!(!is_gemini_4("gemini-3.8-flash"));

    let levels = [
        (Effort::Off, "LOW"),
        (Effort::Low, "LOW"),
        (Effort::Medium, "MEDIUM"),
        (Effort::High, "HIGH"),
    ];
    for (effort, expected) in levels {
        let cfg = thinking_config("gemini-4-argon", effort);
        assert_eq!(cfg["thinkingLevel"], expected);
    }

    let cyber_cfg = thinking_config("gemini-4-argon-cyber-permissive", Effort::Medium);
    assert_eq!(cyber_cfg, json!({ "thinkingLevel": "HIGH" }));

    let mut request = sample_user_request();
    request.temperature = Some(0.7);

    let body = build_request_body(
        RequestTarget {
            project: "p",
            runtime_model: "gemini-4-argon",
            effort: Effort::Medium,
        },
        &request,
        &envelope(),
    )
    .unwrap();

    let gen_cfg = &body["request"]["generationConfig"];
    assert_eq!(gen_cfg["maxOutputTokens"], 65535);
    assert_eq!(gen_cfg["thinkingConfig"]["thinkingLevel"], "MEDIUM");
    assert!(gen_cfg["thinkingConfig"].get("thinkingBudget").is_none());
    assert!(gen_cfg.get("temperature").is_none());
}
