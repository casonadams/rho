//! Modal tool approval prompting helpers.

use crate::ui::interactive::{InteractionOption, InteractionPrompt, InteractionResponse, InteractiveUi};
use rho_harness_core::presentation::ApprovalResult;
use rho_harness_core::presentation::summary::format_tool_args_full;
use serde_json::Value;

pub async fn prompt_interactive_tool_approval(ui: &InteractiveUi, name: &str, arguments: &Value) -> ApprovalResult {
    let prompt = InteractionPrompt {
        title: "Permission Request".to_string(),
        body: approval_body(name, arguments),
        options: vec![
            InteractionOption {
                label: "Allow".to_string(),
                description: Some("Execute this tool call".to_string()),
                input: None,
            },
            InteractionOption {
                label: "Deny".to_string(),
                description: Some("Block this tool call".to_string()),
                input: None,
            },
        ],
        initial_selection: 0,
        allow_custom: false,
        initial_text: None,
    };

    match ui.request(prompt).await {
        Ok(InteractionResponse::Selected(0)) => ApprovalResult::Approved,
        _ => ApprovalResult::Denied {
            reason: "user rejected tool execution".to_string(),
        },
    }
}

/// The modal body: the tool's input verbatim (no JSON braces) for known
/// tools; compact JSON capped at 200 chars for unknown (e.g. MCP) tools.
fn approval_body(name: &str, arguments: &Value) -> String {
    match input_summary(name, arguments) {
        Some(summary) => format!("Tool: {name}\n{summary}"),
        None => format!("Tool: {name}"),
    }
}

fn input_summary(name: &str, arguments: &Value) -> Option<String> {
    let summary = format_tool_args_full(name, arguments);
    if !summary.is_empty() {
        return Some(summary);
    }
    if arguments.as_object().is_none_or(|object| object.is_empty()) {
        return None;
    }
    let text = serde_json::to_string(arguments).unwrap_or_default();
    let capped = if text.chars().count() <= 200 {
        text
    } else {
        format!("{}\u{2026}", text.chars().take(200).collect::<String>())
    };
    (!capped.is_empty()).then_some(capped)
}
