use super::*;

#[test]
fn test_option_layout_default_is_vertical() {
    assert_eq!(OptionLayout::default(), OptionLayout::Vertical);
}

#[test]
fn test_option_layout_serde_roundtrip() {
    let horizontal_json = serde_json::to_string(&OptionLayout::Horizontal).unwrap();
    assert_eq!(horizontal_json, "\"horizontal\"");
    let parsed: OptionLayout = serde_json::from_str("\"horizontal\"").unwrap();
    assert_eq!(parsed, OptionLayout::Horizontal);

    let vertical_json = serde_json::to_string(&OptionLayout::Vertical).unwrap();
    assert_eq!(vertical_json, "\"vertical\"");
    let parsed_vert: OptionLayout = serde_json::from_str("\"vertical\"").unwrap();
    assert_eq!(parsed_vert, OptionLayout::Vertical);
}

#[test]
fn test_interaction_prompt_deserialization_defaults_layout_to_vertical() {
    let json_data = r#"{
        "title": "Perm",
        "body": "test",
        "options": []
    }"#;
    let prompt: InteractionPrompt = serde_json::from_str(json_data).unwrap();
    assert_eq!(prompt.option_layout, OptionLayout::Vertical);
}

#[test]
fn test_interaction_prompt_deserialization_respects_horizontal() {
    let json_data = r#"{
        "title": "Perm",
        "body": "test",
        "options": [],
        "option_layout": "horizontal"
    }"#;
    let prompt: InteractionPrompt = serde_json::from_str(json_data).unwrap();
    assert_eq!(prompt.option_layout, OptionLayout::Horizontal);
}

#[test]
fn test_turn_telemetry_tokens_per_second() {
    assert_eq!(TurnTelemetry::compute_tokens_per_second(100, 0), None);
    assert_eq!(TurnTelemetry::compute_tokens_per_second(100, 100), None);
    assert_eq!(TurnTelemetry::compute_tokens_per_second(0, 2000), None);
    assert_eq!(TurnTelemetry::compute_tokens_per_second(500, 2000), Some(250.0));
}

#[test]
fn test_turn_telemetry_serde_roundtrip() {
    let telemetry = TurnTelemetry {
        timestamp_unix_ms: 1_700_000_000_000,
        turn_elapsed_ms: 2500,
        request_duration_ms: 2000,
        ttft_ms: Some(150),
        input_tokens: 1000,
        output_tokens: 500,
        cache_read_tokens: 4096,
        cache_write_tokens: 0,
        tokens_per_second: Some(250.0),
        cache_miss_tokens: None,
    };
    let json = serde_json::to_string(&telemetry).unwrap();
    let parsed: TurnTelemetry = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, telemetry);
}
