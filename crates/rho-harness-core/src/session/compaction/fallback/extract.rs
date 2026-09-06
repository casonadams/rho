use std::collections::HashSet;

use rig::message::{AssistantContent, Message, ToolResultContent, UserContent};
use serde_json::Value;

use super::SummaryState;

fn process_user_text_facts(text_str: &str, state: &mut SummaryState) {
    if state.goal.is_empty() && !text_str.is_empty() {
        let first_line = text_str.lines().next().unwrap_or("").trim();
        if !first_line.is_empty() {
            state.goal.push(truncate_str(first_line, 120));
        }
    }
    scan_text_lines(text_str, state);
}

fn process_tool_result_facts(result: &rig::message::ToolResult, state: &mut SummaryState) {
    for item in &result.content {
        if let ToolResultContent::Text(text) = item
            && is_error_text(&text.text)
        {
            let err_line = text.text.lines().next().unwrap_or("").trim();
            let err_desc = format!("Tool `{}` error: {}", result.name, truncate_str(err_line, 100));
            if !state.blocked.contains(&err_desc) {
                state.blocked.push(err_desc);
            }
        }
    }
}

fn process_user_content_facts(part: &UserContent, state: &mut SummaryState) {
    match part {
        UserContent::Text(text) => process_user_text_facts(text.text.trim(), state),
        UserContent::ToolResult(result) => process_tool_result_facts(result, state),
        _ => {}
    }
}

fn record_tool_done_item(item: String, (seen_done, state): (&mut HashSet<String>, &mut SummaryState)) {
    if seen_done.insert(item.clone()) && !state.done.contains(&item) {
        state.done.push(item);
    }
}

fn process_tool_call_facts(call: &rig::message::ToolCall, target: (&mut HashSet<String>, &mut SummaryState)) {
    let name = call.function.name.as_str();
    if is_file_mod_tool(name) {
        if let Some(path) = extract_path(&call.function.arguments) {
            record_tool_done_item(format!("Modified `{path}`"), target);
        }
    } else if is_bash_tool(name)
        && let Some(cmd) = extract_command(&call.function.arguments)
    {
        record_tool_done_item(format!("Ran command `{}`", truncate_str(&cmd, 60)), target);
    }
}

fn process_assistant_content_facts(item: &AssistantContent, target: (&mut HashSet<String>, &mut SummaryState)) {
    match item {
        AssistantContent::Text(text) => scan_text_lines(&text.text, target.1),
        AssistantContent::ToolCall(call) => process_tool_call_facts(call, target),
        _ => {}
    }
}

pub fn extract_message_facts(messages: &[Message], state: &mut SummaryState) {
    let mut seen_done = HashSet::new();
    for msg in messages {
        match msg {
            Message::User { content } => {
                for part in content {
                    process_user_content_facts(part, state);
                }
            }
            Message::Assistant { content, .. } => {
                for item in content {
                    process_assistant_content_facts(item, (&mut seen_done, state));
                }
            }
            Message::System { content } => scan_text_lines(content, state),
        }
    }
}

fn is_file_mod_tool(name: &str) -> bool {
    name == "write" || name == "edit" || name.ends_with(":write") || name.ends_with(":edit")
}

fn is_bash_tool(name: &str) -> bool {
    name == "bash" || name.ends_with(":bash")
}

fn record_decision_if_present(trimmed: &str, state: &mut SummaryState) {
    let lower = trimmed.to_ascii_lowercase();
    if lower.contains("decision:") || lower.contains("decided to ") {
        let clean = clean_item(trimmed);
        if !state.decisions.contains(&clean) {
            state.decisions.push(clean);
        }
    }
}

fn scan_single_line(trimmed: &str, state: &mut SummaryState) {
    if let Some(rest) = trimmed.strip_prefix("- [x] ") {
        let item = clean_item(rest);
        if !state.done.contains(&item) {
            state.done.push(item);
        }
    } else if let Some(rest) = trimmed.strip_prefix("- [ ] ") {
        let item = clean_item(rest);
        if !state.in_progress.contains(&item) {
            state.in_progress.push(item);
        }
    } else {
        record_decision_if_present(trimmed, state);
    }
}

fn scan_text_lines(text: &str, state: &mut SummaryState) {
    for line in text.lines() {
        scan_single_line(line.trim(), state);
    }
}

pub fn clean_item(line: &str) -> String {
    let s = line.trim();
    let stripped = if let Some(rest) = s.strip_prefix("- [x] ") {
        rest
    } else if let Some(rest) = s.strip_prefix("- [ ] ") {
        rest
    } else if let Some(rest) = s.strip_prefix("- ") {
        rest
    } else if let Some(rest) = s.strip_prefix("* ") {
        rest
    } else {
        s.trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == ' ')
    };
    stripped.trim().to_string()
}

fn is_error_text(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    ["error", "failed", "denied", "timed out", "panic"]
        .iter()
        .any(|m| lower.contains(m))
}

fn extract_path(args: &Value) -> Option<String> {
    if let Some(obj) = args.as_object() {
        obj.get("path")
            .or_else(|| obj.get("file_path"))
            .or_else(|| obj.get("file"))
            .and_then(|v| v.as_str())
            .map(|p| p.trim().strip_prefix("./").unwrap_or(p.trim()).to_string())
    } else if let Some(s) = args.as_str() {
        if let Ok(val) = serde_json::from_str::<Value>(s) {
            extract_path(&val)
        } else {
            None
        }
    } else {
        None
    }
}

fn extract_command(args: &Value) -> Option<String> {
    if let Some(obj) = args.as_object() {
        obj.get("command")
            .and_then(|v| v.as_str())
            .map(|c| c.trim().to_string())
    } else if let Some(s) = args.as_str() {
        if let Ok(val) = serde_json::from_str::<Value>(s) {
            extract_command(&val)
        } else {
            None
        }
    } else {
        None
    }
}

fn truncate_str(s: &str, max_chars: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let prefix: String = s.chars().take(max_chars).collect();
        format!("{prefix}...")
    }
}
