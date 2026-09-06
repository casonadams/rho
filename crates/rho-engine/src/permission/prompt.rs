use rho_harness_core::presentation::{InteractionInput, InteractionOption, InteractionPrompt, OptionLayout};
use serde_json::Value;

use super::suggest::match_input;
use super::types::RuleDraft;

fn make_option(label: &str, desc: &str, input: Option<InteractionInput>) -> InteractionOption {
    InteractionOption {
        label: label.to_string(),
        description: Some(desc.to_string()),
        input,
    }
}

fn build_permission_options(input_display: String, drafts: &[RuleDraft]) -> Vec<InteractionOption> {
    let always_desc = drafts
        .first()
        .map(|d| format!("Save rule: [{}] \"{}\" = \"allow\"", d.surface, d.pattern))
        .unwrap_or_else(|| "Save allow rule to permission.toml".to_string());
    vec![
        make_option("Allow", "Run this tool call once", None),
        make_option(
            "Edit",
            "Edit tool arguments before running",
            Some(InteractionInput {
                label: "args".to_string(),
                value: Some(input_display),
            }),
        ),
        make_option("Always", &always_desc, None),
        make_option(
            "Deny",
            "Deny tool execution",
            Some(InteractionInput {
                label: "reason".to_string(),
                value: None,
            }),
        ),
    ]
}

pub fn build_permission_prompt(tool: &str, args: &Value, drafts: &[RuleDraft]) -> InteractionPrompt {
    let input_display = match_input(args);
    let body = format!("Tool: {tool}\nInput: {input_display}");
    InteractionPrompt {
        title: "Permission Required".to_string(),
        body,
        options: build_permission_options(input_display, drafts),
        initial_selection: 0,
        allow_custom: false,
        initial_text: None,
        option_layout: OptionLayout::Horizontal,
    }
}

pub fn rewrite_tool_args(original: &Value, edited_text: &str) -> Value {
    if let Ok(val) = serde_json::from_str::<Value>(edited_text) {
        return val;
    }
    if let Value::Object(mut map) = original.clone() {
        for key in &["command", "url", "query", "path", "pattern"] {
            if map.contains_key(*key) {
                map.insert((*key).to_string(), Value::String(edited_text.to_string()));
                return Value::Object(map);
            }
        }
    }
    Value::String(edited_text.to_string())
}
