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
