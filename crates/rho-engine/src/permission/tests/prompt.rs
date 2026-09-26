use crate::permission::prompt::build_permission_prompt;
use rho_harness_core::presentation::OptionLayout;
use serde_json::json;

#[test]
fn test_build_permission_prompt_layout_and_labels() {
    let args = json!({"command": "cargo test"});
    let prompt = build_permission_prompt("bash", &args, &[], None);
    assert_eq!(prompt.option_layout, OptionLayout::Horizontal);
    assert_eq!(prompt.title, "Permission Required");
    assert_eq!(prompt.body, "Tool: bash\nInput: cargo test");
    let labels: Vec<&str> = prompt.options.iter().map(|o| o.label.as_str()).collect();
    assert_eq!(labels, ["Allow", "Edit", "Always", "Deny"]);
}

#[test]
fn test_build_permission_prompt_with_notice_order() {
    let args = json!({"command": "mkdir /tmp/test && rm -rf /tmp/test"});
    let prompt = build_permission_prompt(
        "bash",
        &args,
        &[],
        Some("Combines safe directory creation with destructive directory deletion"),
    );
    assert_eq!(
        prompt.body,
        "Tool: bash\nNotice: Combines safe directory creation with destructive directory deletion\nInput: mkdir /tmp/test &&\n  rm -rf /tmp/test"
    );
}

#[test]
fn test_build_permission_prompt_inputs() {
    let args = json!({"command": "cargo test"});
    let prompt = build_permission_prompt("bash", &args, &[], None);
    let input_labels: Vec<Option<&str>> = prompt
        .options
        .iter()
        .map(|o| o.input.as_ref().map(|i| i.label.as_str()))
        .collect();
    assert_eq!(input_labels, [None, Some("edit"), Some("pattern"), Some("reason")]);
    let pattern_value = prompt.options[2].input.as_ref().and_then(|i| i.value.as_deref());
    assert_eq!(pattern_value, Some("cargo test *"));
}

#[test]
fn test_build_permission_prompt_formats_bash_commands_at_seams() {
    let args = json!({"command": "git status && cargo test || echo failed ; ls"});
    let prompt = build_permission_prompt("bash", &args, &[], None);
    let expected = "git status &&\n  cargo test ||\n  echo failed;\nls";
    assert!(prompt.body.contains(expected));
    let edit_value = prompt.options[1].input.as_ref().and_then(|i| i.value.as_deref());
    assert_eq!(edit_value, Some(expected));
}
