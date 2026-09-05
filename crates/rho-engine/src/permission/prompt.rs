use rho_harness_core::presentation::{InteractionInput, InteractionOption, InteractionPrompt};
use serde_json::Value;

use super::suggest::match_input;
use super::types::RuleDraft;

pub fn build_permission_prompt(tool: &str, args: &Value, drafts: &[RuleDraft]) -> InteractionPrompt {
    let input_display = match_input(args);
    let title = "Permission Required".to_string();
    let body = format!("Tool: {tool}\nInput: {input_display}");

    let always_allow_desc = drafts
        .first()
        .map(|draft| format!("Save rule: [{}] \"{}\" = \"allow\"", draft.surface, draft.pattern));

    let options = vec![
        InteractionOption {
            label: "Allow".to_string(),
            description: Some("Run this tool call once".to_string()),
            input: None,
        },
        InteractionOption {
            label: "Edit".to_string(),
            description: Some("Edit tool arguments before running".to_string()),
            input: Some(InteractionInput {
                label: "args".to_string(),
                value: Some(input_display),
            }),
        },
        InteractionOption {
            label: "Always allow".to_string(),
            description: always_allow_desc.or_else(|| Some("Save allow rule to permission.toml".to_string())),
            input: None,
        },
        InteractionOption {
            label: "Deny with reason".to_string(),
            description: Some("Deny tool execution".to_string()),
            input: Some(InteractionInput {
                label: "reason".to_string(),
                value: None,
            }),
        },
    ];

    InteractionPrompt {
        title,
        body,
        options,
        initial_selection: 0,
        allow_custom: false,
        initial_text: None,
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
