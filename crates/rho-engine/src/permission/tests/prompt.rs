use crate::permission::prompt::build_permission_prompt;
use rho_harness_core::presentation::OptionLayout;
use serde_json::json;

#[test]
fn test_build_permission_prompt_layout_and_labels() {
    let args = json!({"command": "cargo test"});
    let prompt = build_permission_prompt("bash", &args, &[]);
    assert_eq!(prompt.option_layout, OptionLayout::Horizontal);
    assert_eq!(prompt.title, "Permission Required");
    let labels: Vec<&str> = prompt.options.iter().map(|o| o.label.as_str()).collect();
    assert_eq!(labels, ["Allow", "Edit", "Always", "Deny"]);
}

#[test]
fn test_build_permission_prompt_inputs() {
    let args = json!({"command": "cargo test"});
    let prompt = build_permission_prompt("bash", &args, &[]);
    let input_labels: Vec<Option<&str>> = prompt
        .options
        .iter()
        .map(|o| o.input.as_ref().map(|i| i.label.as_str()))
        .collect();
    assert_eq!(input_labels, [None, Some("args"), None, Some("reason")]);
}

#[test]
fn test_build_permission_prompt_formats_bash_commands_at_seams() {
    let args = json!({"command": "git status && cargo test || echo failed ; ls"});
    let prompt = build_permission_prompt("bash", &args, &[]);
    let expected = "git status &&\n  cargo test ||\n  echo failed;\nls";
    assert!(prompt.body.contains(expected));
    let edit_value = prompt.options[1].input.as_ref().and_then(|i| i.value.as_deref());
    assert_eq!(edit_value, Some(expected));
}
