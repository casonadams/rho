//! Artifact construction: pull critical facts out of evicted messages and
//! bound the resulting summary string.
//!
//! Extracted from `session/context.rs` during the file-length refactor.

use std::collections::HashSet;

use rig::message::{AssistantContent, Message, ToolResultContent, UserContent};

pub(super) struct ArtifactParams<'a> {
    pub(super) carry: Option<&'a str>,
    pub(super) messages: &'a [Message],
    pub(super) template: &'a str,
    pub(super) max_bytes: usize,
}

pub(super) fn build_artifact(params: ArtifactParams<'_>) -> String {
    let facts = critical_facts(params.carry, params.messages);
    let mut output = String::new();
    push_bounded_line(&mut output, "[Coding context summary]", params.max_bytes);
    for fact in facts {
        push_bounded_line(&mut output, &fact, params.max_bytes);
    }
    push_bounded_line(&mut output, "Recent compacted transcript:", params.max_bytes);
    append_bounded_suffix(&mut output, params.template, params.max_bytes);
    output
}

fn critical_facts(carry: Option<&str>, messages: &[Message]) -> Vec<String> {
    let mut facts = Vec::new();
    let mut seen = HashSet::new();
    if let Some(previous) = carry {
        for line in previous.lines().filter(|line| is_critical_text(line)) {
            insert_fact(&mut facts, &mut seen, line.trim().to_string());
        }
    }
    for message in messages {
        collect_message_facts(message, &mut facts, &mut seen);
    }
    facts
}

fn collect_message_facts(message: &Message, facts: &mut Vec<String>, seen: &mut HashSet<String>) {
    match message {
        Message::User { content } => {
            for part in content {
                match part {
                    UserContent::Text(text) => collect_text_facts(&text.text, facts, seen),
                    UserContent::ToolResult(result) => {
                        for content in &result.content {
                            let ToolResultContent::Text(text) = content else {
                                continue;
                            };
                            if result.name == "bash" || is_error_text(&text.text) {
                                insert_fact(
                                    facts,
                                    seen,
                                    format!("tool result ({}): {}", result.name, text.text.trim()),
                                );
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
                    AssistantContent::Text(text) => collect_text_facts(&text.text, facts, seen),
                    AssistantContent::ToolCall(call) => collect_tool_call_facts(call, facts, seen),
                    _ => {}
                }
            }
        }
        Message::System { content } => collect_text_facts(content, facts, seen),
    }
}

fn collect_tool_call_facts(call: &rig::message::ToolCall, facts: &mut Vec<String>, seen: &mut HashSet<String>) {
    match call.function.name.as_str() {
        "write" | "edit" => {
            if let Some(path) = call.function.arguments.get("path").and_then(serde_json::Value::as_str) {
                insert_fact(facts, seen, format!("changed file: {path}"));
            }
        }
        "bash" => {
            if let Some(command) = call
                .function
                .arguments
                .get("command")
                .and_then(serde_json::Value::as_str)
            {
                insert_fact(facts, seen, format!("verification command: {command}"));
            }
        }
        _ => {}
    }
}

fn collect_text_facts(text: &str, facts: &mut Vec<String>, seen: &mut HashSet<String>) {
    for line in text.lines().filter(|line| is_critical_text(line)) {
        insert_fact(facts, seen, line.trim().to_string());
    }
}

fn insert_fact(facts: &mut Vec<String>, seen: &mut HashSet<String>, fact: String) {
    if !fact.is_empty() && seen.insert(fact.clone()) {
        facts.push(fact);
    }
}

fn is_critical_text(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    [
        "objective",
        "constraint",
        "decision",
        "changed file",
        "verification",
        "test",
        "error",
        "failed",
        "unresolved",
        "remaining",
        "todo",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

fn is_error_text(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    ["error", "failed", "denied", "timed out", "not found"]
        .iter()
        .any(|marker| lower.contains(marker))
}

fn push_bounded_line(output: &mut String, line: &str, max_bytes: usize) {
    if output.len() >= max_bytes {
        return;
    }
    let available = max_bytes.saturating_sub(output.len() + 1);
    let end = char_boundary_at_most(line, available);
    output.push_str(line.get(..end).unwrap_or_default());
    output.push('\n');
}

fn append_bounded_suffix(output: &mut String, text: &str, max_bytes: usize) {
    let available = max_bytes.saturating_sub(output.len());
    if available == 0 {
        return;
    }
    let start = char_boundary_at_least(text, text.len().saturating_sub(available));
    output.push_str(text.get(start..).unwrap_or_default());
}

pub(super) fn char_boundary_at_most(text: &str, mut index: usize) -> usize {
    index = index.min(text.len());
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

pub(super) fn char_boundary_at_least(text: &str, mut index: usize) -> usize {
    index = index.min(text.len());
    while index < text.len() && !text.is_char_boundary(index) {
        index += 1;
    }
    index
}
