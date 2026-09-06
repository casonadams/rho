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

fn make_input(label: &str, value: Option<String>) -> Option<InteractionInput> {
    Some(InteractionInput {
        label: label.to_string(),
        value,
    })
}

fn always_allow_spec(tool: &str, input_display: &str, drafts: &[RuleDraft]) -> (String, String) {
    let pattern = drafts
        .first()
        .map(|d| d.pattern.clone())
        .unwrap_or_else(|| super::suggest::suggested_rule(tool, input_display));
    let desc = drafts
        .first()
        .map(|d| format!("Save rule: [{}] \"{}\" = \"allow\"", d.surface, d.pattern))
        .unwrap_or_else(|| {
            format!(
                "Save rule: [{}] \"{pattern}\" = \"allow\"",
                super::suggest::canonical_tool(tool)
            )
        });
    (pattern, desc)
}

struct PermissionPromptParams<'a> {
    tool: &'a str,
    formatted: String,
    drafts: &'a [RuleDraft],
}

fn build_permission_options(p: PermissionPromptParams<'_>, input_display: &str) -> Vec<InteractionOption> {
    let (pattern, desc) = always_allow_spec(p.tool, input_display, p.drafts);
    vec![
        make_option("Allow", "Run this tool call once", None),
        make_option(
            "Edit",
            "Edit tool arguments before running",
            make_input("args", Some(p.formatted)),
        ),
        make_option("Always", &desc, make_input("pattern", Some(pattern))),
        make_option("Deny", "Deny tool execution", make_input("reason", None)),
    ]
}

pub fn build_permission_prompt(tool: &str, args: &Value, drafts: &[RuleDraft]) -> InteractionPrompt {
    let input_display = match_input(args);
    let formatted_input = if tool == "bash" {
        super::bash::format_command_lines(&input_display)
    } else {
        input_display.clone()
    };
    let body = format!("Tool: {tool}\nInput: {formatted_input}");
    let params = PermissionPromptParams {
        tool,
        formatted: formatted_input,
        drafts,
    };
    InteractionPrompt {
        title: "Permission Required".to_string(),
        body,
        options: build_permission_options(params, &input_display),
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
