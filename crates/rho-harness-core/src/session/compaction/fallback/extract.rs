use std::collections::HashSet;

use rig::message::{AssistantContent, Message, ToolResultContent, UserContent};
use serde_json::Value;

use super::SummaryState;

pub fn extract_message_facts(messages: &[Message], state: &mut SummaryState) {
    let mut seen_done = HashSet::new();

    for msg in messages {
        match msg {
            Message::User { content } => {
                for part in content {
                    match part {
                        UserContent::Text(text) => {
                            let text_str = text.text.trim();
                            if state.goal.is_empty() && !text_str.is_empty() {
                                let first_line = text_str.lines().next().unwrap_or("").trim();
                                if !first_line.is_empty() {
                                    state.goal.push(truncate_str(first_line, 120));
                                }
                            }
                            scan_text_lines(text_str, state);
                        }
                        UserContent::ToolResult(result) => {
                            for item in &result.content {
                                if let ToolResultContent::Text(text) = item
                                    && is_error_text(&text.text)
                                {
                                    let err_line = text.text.lines().next().unwrap_or("").trim();
                                    let err_desc =
                                        format!("Tool `{}` error: {}", result.name, truncate_str(err_line, 100));
                                    if !state.blocked.contains(&err_desc) {
                                        state.blocked.push(err_desc);
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            Message::Assistant { content, .. } => {
                for part in content {
                    match part {
                        AssistantContent::Text(text) => {
                            scan_text_lines(&text.text, state);
                        }
                        AssistantContent::ToolCall(call) => {
                            let name = call.function.name.as_str();
                            if is_file_mod_tool(name) {
                                if let Some(path) = extract_path(&call.function.arguments) {
                                    let item = format!("Modified `{path}`");
                                    if seen_done.insert(item.clone()) && !state.done.contains(&item) {
                                        state.done.push(item);
                                    }
                                }
                            } else if is_bash_tool(name)
                                && let Some(cmd) = extract_command(&call.function.arguments)
                            {
                                let item = format!("Ran command `{}`", truncate_str(&cmd, 60));
                                if seen_done.insert(item.clone()) && !state.done.contains(&item) {
                                    state.done.push(item);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            Message::System { content } => {
                scan_text_lines(content, state);
            }
        }
    }
}

fn is_file_mod_tool(name: &str) -> bool {
    name == "write" || name == "edit" || name.ends_with(":write") || name.ends_with(":edit")
}

fn is_bash_tool(name: &str) -> bool {
    name == "bash" || name.ends_with(":bash")
}

fn scan_text_lines(text: &str, state: &mut SummaryState) {
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("- [x] ") {
            let item = clean_item(trimmed);
            if !state.done.contains(&item) {
                state.done.push(item);
            }
        } else if trimmed.starts_with("- [ ] ") {
            let item = clean_item(trimmed);
            if !state.in_progress.contains(&item) {
                state.in_progress.push(item);
            }
        } else {
            let lower = trimmed.to_ascii_lowercase();
            if lower.contains("decision:") || lower.contains("decided to ") {
                let clean = clean_item(trimmed);
                if !state.decisions.contains(&clean) {
                    state.decisions.push(clean);
                }
            }
        }
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
