use std::collections::HashMap;

use rho_harness_core::tokens::cut_point::is_user_turn_start;
use rig::message::{AssistantContent, Message, ToolResultContent, UserContent};

pub const DEFAULT_PRUNE_LINE_THRESHOLD: usize = 15;

fn is_assistant_final_response(message: &Message) -> bool {
    match message {
        Message::Assistant { content, .. } => {
            let has_tool_calls = content.iter().any(|c| matches!(c, AssistantContent::ToolCall(_)));
            let has_text = content
                .iter()
                .any(|c| matches!(c, AssistantContent::Text(t) if !t.text.trim().is_empty()));
            !has_tool_calls && has_text
        }
        _ => false,
    }
}

pub fn is_turn_boundary(message: &Message) -> bool {
    is_user_turn_start(message) || is_assistant_final_response(message)
}

pub fn find_turn_boundary_cutoff(messages: &[Message], volatile_turns: usize) -> usize {
    if volatile_turns == 0 || messages.is_empty() {
        return messages.len();
    }

    let mut remaining = volatile_turns;
    for (idx, msg) in messages.iter().enumerate().rev() {
        if is_turn_boundary(msg) {
            remaining = remaining.saturating_sub(1);
            if remaining == 0 {
                return idx;
            }
        }
    }
    0
}

struct BashPruneDetails {
    lines: usize,
    size_str: String,
    log_path: Option<String>,
}

fn parse_bash_footer_details(text: &str) -> Option<BashPruneDetails> {
    let footer_start = text.rfind("[Command completed successfully with exit code 0 (")?;
    let footer = &text[footer_start..];
    let close = footer.find(']')?;
    let inner = &footer[..close];

    let lines_size_start = inner.find('(')?;
    let lines_size_end = inner.find(')')?;
    let parts = &inner[lines_size_start + 1..lines_size_end];
    let mut split = parts.split(',');
    let lines_part = split.next()?.trim();
    let size_part = split.next()?.trim();

    let lines = lines_part.split_whitespace().next()?.parse::<usize>().ok()?;

    let log_path = if let Some(path_start) = inner.find("Full log:") {
        let after = inner[path_start + "Full log:".len()..].trim();
        let path = after.split_whitespace().next()?.trim_matches(']');
        Some(path.to_string())
    } else {
        None
    };

    Some(BashPruneDetails {
        lines,
        size_str: size_part.to_string(),
        log_path,
    })
}

fn check_prunable_bash_output(text: &str, line_threshold: usize) -> Option<BashPruneDetails> {
    if text.contains("Command exited with code") || text.contains("Command timed out after") {
        return None;
    }

    if let Some(details) = parse_bash_footer_details(text) {
        if details.lines > line_threshold {
            return Some(details);
        }
        return None;
    }

    let line_count = text.lines().count();
    if line_count > line_threshold {
        Some(BashPruneDetails {
            lines: line_count,
            size_str: crate::tools::truncate::format_size(text.len()),
            log_path: None,
        })
    } else {
        None
    }
}

#[derive(Debug, Clone)]
struct ToolCallMeta {
    name: String,
    target: String,
}

fn collect_tool_calls(messages: &[Message]) -> HashMap<String, ToolCallMeta> {
    let mut map = HashMap::new();
    for msg in messages {
        if let Message::Assistant { content, .. } = msg {
            for item in content {
                if let AssistantContent::ToolCall(call) = item {
                    let name = call.function.name.to_ascii_lowercase();
                    let target = match name.as_str() {
                        "bash" => call
                            .function
                            .arguments
                            .get("command")
                            .and_then(|v| v.as_str())
                            .unwrap_or("bash")
                            .to_string(),
                        "read" | "read_file" | "write" | "write_file" | "edit" | "edit_file" => call
                            .function
                            .arguments
                            .get("path")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        "rg" | "grep" | "fd" | "find" | "glob" => call
                            .function
                            .arguments
                            .get("pattern")
                            .and_then(|v| v.as_str())
                            .or_else(|| call.function.arguments.get("query").and_then(|v| v.as_str()))
                            .unwrap_or("")
                            .to_string(),
                        "web_fetch" | "webfetch" => call
                            .function
                            .arguments
                            .get("url")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        "web_search" | "websearch" => call
                            .function
                            .arguments
                            .get("query")
                            .or_else(|| call.function.arguments.get("pattern"))
                            .or_else(|| call.function.arguments.get("q"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        _ => String::new(),
                    };
                    map.insert(call.id.to_string(), ToolCallMeta { name, target });
                }
            }
        }
    }
    map
}

fn format_pruned_stub(cmd: &str, details: &BashPruneDetails) -> String {
    match &details.log_path {
        Some(path) => format!(
            "[Command '{cmd}' completed with exit code 0. Output pruned ({} lines, {}). Full log: {path}]",
            details.lines, details.size_str
        ),
        None => format!(
            "[Command '{cmd}' completed with exit code 0. Output pruned ({} lines, {}).]",
            details.lines, details.size_str
        ),
    }
}

fn is_read_error(text: &str) -> bool {
    text.starts_with("Error")
        || text.contains("File not found")
        || text.contains("Failed to read")
        || text.contains("Empty file path")
        || text.contains("File contains invalid UTF-8")
        || (text.starts_with("Offset ") && text.contains("is beyond end of file"))
}

fn is_search_error(text: &str) -> bool {
    text.starts_with("Error") || text.starts_with("Search timed out") || text.starts_with("Failed ")
}

fn is_fetch_error(text: &str) -> bool {
    text.starts_with("Error") || text.starts_with("Empty URL") || text.starts_with("Failed ")
}

fn prune_bash_result(text: &str, meta: Option<&ToolCallMeta>, line_threshold: usize) -> Option<String> {
    let details = check_prunable_bash_output(text, line_threshold)?;
    let cmd = meta.map(|m| m.target.as_str()).unwrap_or("bash");
    Some(format_pruned_stub(cmd, &details))
}

fn prune_read_result(text: &str, meta: Option<&ToolCallMeta>, line_threshold: usize) -> Option<String> {
    if is_read_error(text) {
        return None;
    }
    let line_count = text.lines().count();
    if line_count <= line_threshold {
        return None;
    }
    let size_str = crate::tools::truncate::format_size(text.len());
    let target = meta.map(|m| m.target.as_str()).unwrap_or("");
    if target.is_empty() {
        Some(format!(
            "[File read ({line_count} lines, {size_str}). Output pruned for historical turn.]"
        ))
    } else {
        Some(format!(
            "[File '{target}' read ({line_count} lines, {size_str}). Output pruned for historical turn.]"
        ))
    }
}

fn prune_search_result(
    tool_name: &str,
    text: &str,
    meta: Option<&ToolCallMeta>,
    line_threshold: usize,
) -> Option<String> {
    if is_search_error(text) {
        return None;
    }
    let line_count = text.lines().count();
    if line_count <= line_threshold {
        return None;
    }
    let size_str = crate::tools::truncate::format_size(text.len());
    let target = meta.map(|m| m.target.as_str()).unwrap_or("");
    if target.is_empty() {
        Some(format!(
            "[Tool '{tool_name}' completed with {line_count} lines ({size_str}). Output pruned for historical turn.]"
        ))
    } else {
        Some(format!(
            "[Tool '{tool_name}' with pattern '{target}' completed with {line_count} lines ({size_str}). Output pruned for historical turn.]"
        ))
    }
}

fn prune_web_result(tool_name: &str, text: &str, meta: Option<&ToolCallMeta>, line_threshold: usize) -> Option<String> {
    let line_count = text.lines().count();
    if line_count <= line_threshold {
        return None;
    }
    let size_str = crate::tools::truncate::format_size(text.len());
    let target = meta.map(|m| m.target.as_str()).unwrap_or("");
    if tool_name == "web_fetch" || tool_name == "webfetch" {
        if is_fetch_error(text) {
            return None;
        }
        if target.is_empty() {
            Some(format!(
                "[URL fetched ({line_count} lines, {size_str}). Output pruned for historical turn.]"
            ))
        } else {
            Some(format!(
                "[URL '{target}' fetched ({line_count} lines, {size_str}). Output pruned for historical turn.]"
            ))
        }
    } else if tool_name == "web_search" || tool_name == "websearch" {
        if is_search_error(text) {
            return None;
        }
        if target.is_empty() {
            Some(format!(
                "[Web search completed with {line_count} lines ({size_str}). Output pruned for historical turn.]"
            ))
        } else {
            Some(format!(
                "[Web search for '{target}' completed with {line_count} lines ({size_str}). Output pruned for historical turn.]"
            ))
        }
    } else {
        None
    }
}

fn prune_mcp_result(tool_name: &str, text: &str, line_threshold: usize) -> Option<String> {
    if text.starts_with("Error") || text.starts_with("Failed ") {
        return None;
    }
    let line_count = text.lines().count();
    if line_count <= line_threshold {
        return None;
    }
    let size_str = crate::tools::truncate::format_size(text.len());
    Some(format!(
        "[Tool '{tool_name}' completed with {line_count} lines ({size_str}). Output pruned for historical turn.]"
    ))
}

fn prune_tool_result_item(
    item: &UserContent,
    tool_calls: &HashMap<String, ToolCallMeta>,
    line_threshold: usize,
) -> Option<UserContent> {
    let UserContent::ToolResult(res) = item else {
        return None;
    };
    let meta = tool_calls.get(&res.call.to_string());
    let tool_name = if !res.name.is_empty() {
        res.name.to_ascii_lowercase()
    } else {
        meta.map_or(String::new(), |m| m.name.clone())
    };

    let text = res
        .content
        .iter()
        .filter_map(|c| match c {
            ToolResultContent::Text(t) => Some(t.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");

    let stub = if tool_name == "bash" {
        prune_bash_result(&text, meta, line_threshold)?
    } else if tool_name == "read" || tool_name == "read_file" {
        prune_read_result(&text, meta, line_threshold)?
    } else if matches!(tool_name.as_str(), "rg" | "grep" | "fd" | "find" | "glob") {
        prune_search_result(&tool_name, &text, meta, line_threshold)?
    } else if tool_name.starts_with("web_") || tool_name.starts_with("web") {
        prune_web_result(&tool_name, &text, meta, line_threshold)?
    } else if tool_name.starts_with("mcp__") || tool_name.contains("__") {
        prune_mcp_result(&tool_name, &text, line_threshold)?
    } else {
        return None;
    };

    Some(UserContent::ToolResult(rig::message::ToolResult {
        call: res.call.clone(),
        provider: res.provider.clone(),
        name: res.name.clone(),
        content: vec![ToolResultContent::text(stub)],
    }))
}

fn prune_write_call(call: &rig::message::ToolCall, line_threshold: usize) -> Option<rig::message::ToolCall> {
    let content_str = call.function.arguments.get("content")?.as_str()?;
    let line_count = content_str.lines().count();
    let bytes = content_str.len();
    if line_count <= line_threshold && bytes <= 500 {
        return None;
    }
    let size_str = crate::tools::truncate::format_size(bytes);
    let stub = format!("[Content written ({line_count} lines, {size_str})]");
    let mut new_args = call.function.arguments.clone();
    new_args["content"] = serde_json::Value::String(stub);

    Some(rig::message::ToolCall::new(
        call.id.clone(),
        rig::message::ToolFunction::new(call.function.name.clone(), new_args),
    ))
}

fn prune_edit_chunk(text: &str, label: &str, line_threshold: usize) -> Option<String> {
    let lines = text.lines().count();
    let bytes = text.len();
    if lines > line_threshold || bytes > 500 {
        let size_str = crate::tools::truncate::format_size(bytes);
        Some(format!("[{label} ({lines} lines, {size_str})]"))
    } else {
        None
    }
}

fn prune_edit_call(call: &rig::message::ToolCall, line_threshold: usize) -> Option<rig::message::ToolCall> {
    let edits = call.function.arguments.get("edits")?.as_array()?;
    let mut modified = false;
    let new_edits: Vec<serde_json::Value> = edits
        .iter()
        .map(|edit| {
            let mut edit_obj = match edit.as_object() {
                Some(obj) => obj.clone(),
                None => return edit.clone(),
            };
            if let Some(old_str) = edit_obj.get("oldText").and_then(|v| v.as_str())
                && let Some(stub) = prune_edit_chunk(old_str, "Old text", line_threshold)
            {
                modified = true;
                edit_obj.insert("oldText".to_string(), serde_json::Value::String(stub));
            }
            if let Some(new_str) = edit_obj.get("newText").and_then(|v| v.as_str())
                && let Some(stub) = prune_edit_chunk(new_str, "New text", line_threshold)
            {
                modified = true;
                edit_obj.insert("newText".to_string(), serde_json::Value::String(stub));
            }
            serde_json::Value::Object(edit_obj)
        })
        .collect();

    if modified {
        let mut new_args = call.function.arguments.clone();
        new_args["edits"] = serde_json::Value::Array(new_edits);
        Some(rig::message::ToolCall::new(
            call.id.clone(),
            rig::message::ToolFunction::new(call.function.name.clone(), new_args),
        ))
    } else {
        None
    }
}

fn prune_assistant_item(item: &AssistantContent, line_threshold: usize) -> Option<AssistantContent> {
    let AssistantContent::ToolCall(call) = item else {
        return None;
    };
    let name = call.function.name.to_ascii_lowercase();
    if name == "write" || name == "write_file" {
        return prune_write_call(call, line_threshold).map(AssistantContent::ToolCall);
    }
    if name == "edit" || name == "edit_file" {
        return prune_edit_call(call, line_threshold).map(AssistantContent::ToolCall);
    }
    None
}

fn prune_assistant_message_content(
    content: &[AssistantContent],
    line_threshold: usize,
) -> Option<Vec<AssistantContent>> {
    let mut modified = false;
    let new_content: Vec<AssistantContent> = content
        .iter()
        .map(|item| match prune_assistant_item(item, line_threshold) {
            Some(pruned) => {
                modified = true;
                pruned
            }
            None => item.clone(),
        })
        .collect();

    if modified { Some(new_content) } else { None }
}

fn prune_user_message_content(
    content: &[UserContent],
    tool_calls: &HashMap<String, ToolCallMeta>,
    line_threshold: usize,
) -> Option<Vec<UserContent>> {
    let mut modified = false;
    let new_content: Vec<UserContent> = content
        .iter()
        .map(|item| match prune_tool_result_item(item, tool_calls, line_threshold) {
            Some(pruned) => {
                modified = true;
                pruned
            }
            None => item.clone(),
        })
        .collect();

    if modified { Some(new_content) } else { None }
}

pub fn prune_historical_tool_outputs(
    messages: &[Message],
    volatile_turns: usize,
    line_threshold: usize,
) -> Vec<Message> {
    if messages.is_empty() {
        return Vec::new();
    }

    let cutoff_idx = find_turn_boundary_cutoff(messages, volatile_turns);
    if cutoff_idx == 0 {
        return messages.to_vec();
    }

    let tool_calls = collect_tool_calls(messages);
    messages
        .iter()
        .enumerate()
        .map(|(idx, msg)| {
            if idx >= cutoff_idx {
                return msg.clone();
            }
            match msg {
                Message::User { content } => match prune_user_message_content(content, &tool_calls, line_threshold) {
                    Some(new_content) => Message::User { content: new_content },
                    None => msg.clone(),
                },
                Message::Assistant { id, content } => match prune_assistant_message_content(content, line_threshold) {
                    Some(new_content) => Message::Assistant {
                        id: id.clone(),
                        content: new_content,
                    },
                    None => msg.clone(),
                },
                _ => msg.clone(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rig::message::{AssistantContent, Text, ToolCall, ToolCallId, ToolFunction};

    fn make_tool_turn(cid: &str, tool: &str, cmd: &str, tool_output: &str) -> (Message, Message) {
        let call = ToolCall::new(
            ToolCallId::new_or_mint(cid),
            ToolFunction::new(tool.to_string(), serde_json::json!({ "command": cmd })),
        );
        let res = rig::message::ToolResult {
            call: ToolCallId::new_or_mint(cid),
            provider: None,
            name: tool.to_string(),
            content: vec![ToolResultContent::Text(Text::new(tool_output))],
        };
        (
            Message::Assistant {
                id: None,
                content: vec![AssistantContent::ToolCall(call)],
            },
            Message::User {
                content: vec![UserContent::ToolResult(res)],
            },
        )
    }

    fn make_read_turn(cid: &str, path: &str, tool_output: &str) -> (Message, Message) {
        let call = ToolCall::new(
            ToolCallId::new_or_mint(cid),
            ToolFunction::new("read".to_string(), serde_json::json!({ "path": path })),
        );
        let res = rig::message::ToolResult {
            call: ToolCallId::new_or_mint(cid),
            provider: None,
            name: "read".to_string(),
            content: vec![ToolResultContent::Text(Text::new(tool_output))],
        };
        (
            Message::Assistant {
                id: None,
                content: vec![AssistantContent::ToolCall(call)],
            },
            Message::User {
                content: vec![UserContent::ToolResult(res)],
            },
        )
    }

    fn make_search_turn(cid: &str, tool: &str, pattern: &str, tool_output: &str) -> (Message, Message) {
        let call = ToolCall::new(
            ToolCallId::new_or_mint(cid),
            ToolFunction::new(tool.to_string(), serde_json::json!({ "pattern": pattern })),
        );
        let res = rig::message::ToolResult {
            call: ToolCallId::new_or_mint(cid),
            provider: None,
            name: tool.to_string(),
            content: vec![ToolResultContent::Text(Text::new(tool_output))],
        };
        (
            Message::Assistant {
                id: None,
                content: vec![AssistantContent::ToolCall(call)],
            },
            Message::User {
                content: vec![UserContent::ToolResult(res)],
            },
        )
    }

    fn make_write_turn(cid: &str, path: &str, content: &str, result_msg: &str) -> (Message, Message) {
        let call = ToolCall::new(
            ToolCallId::new_or_mint(cid),
            ToolFunction::new(
                "write".to_string(),
                serde_json::json!({ "path": path, "content": content }),
            ),
        );
        let res = rig::message::ToolResult {
            call: ToolCallId::new_or_mint(cid),
            provider: None,
            name: "write".to_string(),
            content: vec![ToolResultContent::Text(Text::new(result_msg))],
        };
        (
            Message::Assistant {
                id: None,
                content: vec![AssistantContent::ToolCall(call)],
            },
            Message::User {
                content: vec![UserContent::ToolResult(res)],
            },
        )
    }

    fn make_edit_turn(cid: &str, path: &str, edits: Vec<(&str, &str)>, result_msg: &str) -> (Message, Message) {
        let edits_json: Vec<_> = edits
            .into_iter()
            .map(|(old, new)| serde_json::json!({ "oldText": old, "newText": new }))
            .collect();
        let call = ToolCall::new(
            ToolCallId::new_or_mint(cid),
            ToolFunction::new(
                "edit".to_string(),
                serde_json::json!({ "path": path, "edits": edits_json }),
            ),
        );
        let res = rig::message::ToolResult {
            call: ToolCallId::new_or_mint(cid),
            provider: None,
            name: "edit".to_string(),
            content: vec![ToolResultContent::Text(Text::new(result_msg))],
        };
        (
            Message::Assistant {
                id: None,
                content: vec![AssistantContent::ToolCall(call)],
            },
            Message::User {
                content: vec![UserContent::ToolResult(res)],
            },
        )
    }

    fn make_fetch_turn(cid: &str, url: &str, tool_output: &str) -> (Message, Message) {
        let call = ToolCall::new(
            ToolCallId::new_or_mint(cid),
            ToolFunction::new("web_fetch".to_string(), serde_json::json!({ "url": url })),
        );
        let res = rig::message::ToolResult {
            call: ToolCallId::new_or_mint(cid),
            provider: None,
            name: "web_fetch".to_string(),
            content: vec![ToolResultContent::Text(Text::new(tool_output))],
        };
        (
            Message::Assistant {
                id: None,
                content: vec![AssistantContent::ToolCall(call)],
            },
            Message::User {
                content: vec![UserContent::ToolResult(res)],
            },
        )
    }

    fn make_mcp_turn(cid: &str, tool_name: &str, tool_output: &str) -> (Message, Message) {
        let call = ToolCall::new(
            ToolCallId::new_or_mint(cid),
            ToolFunction::new(tool_name.to_string(), serde_json::json!({})),
        );
        let res = rig::message::ToolResult {
            call: ToolCallId::new_or_mint(cid),
            provider: None,
            name: tool_name.to_string(),
            content: vec![ToolResultContent::Text(Text::new(tool_output))],
        };
        (
            Message::Assistant {
                id: None,
                content: vec![AssistantContent::ToolCall(call)],
            },
            Message::User {
                content: vec![UserContent::ToolResult(res)],
            },
        )
    }

    #[test]
    fn test_current_turn_bash_output_is_not_pruned() {
        let output = (1..=30).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        let output_with_footer = format!(
            "{output}\n\n[Command completed successfully with exit code 0 (30 lines, 300B). Full log: /tmp/log.txt]"
        );
        let (call_msg, res_msg) = make_tool_turn("c1", "bash", "cargo test", &output_with_footer);

        let history = vec![Message::user("Please test"), call_msg, res_msg];

        let pruned = prune_historical_tool_outputs(&history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);
        assert_eq!(pruned.len(), 3);

        let Message::User { content } = &pruned[2] else {
            panic!()
        };
        let UserContent::ToolResult(res) = &content[0] else {
            panic!()
        };
        let text = match &res.content[0] {
            ToolResultContent::Text(t) => &t.text,
            _ => panic!(),
        };
        assert!(text.contains("line 1"));
        assert!(text.contains("line 30"));
    }

    #[test]
    fn test_prior_turn_successful_verbose_bash_output_is_pruned() {
        let output = (1..=30).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        let output_with_footer = format!(
            "{output}\n\n[Command completed successfully with exit code 0 (30 lines, 300B). Full log: /tmp/log.txt]"
        );
        let (call_msg, res_msg) = make_tool_turn("c1", "bash", "cargo test", &output_with_footer);

        let history = vec![
            Message::user("Please test"),
            call_msg,
            res_msg,
            Message::assistant("Tests passed successfully!"),
            Message::user("Now run clippy"),
        ];

        let pruned = prune_historical_tool_outputs(&history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);
        assert_eq!(pruned.len(), 5);

        let Message::User { content } = &pruned[2] else {
            panic!()
        };
        let UserContent::ToolResult(res) = &content[0] else {
            panic!()
        };
        let text = match &res.content[0] {
            ToolResultContent::Text(t) => &t.text,
            _ => panic!(),
        };
        assert_eq!(
            text,
            "[Command 'cargo test' completed with exit code 0. Output pruned (30 lines, 300B). Full log: /tmp/log.txt]"
        );
    }

    #[test]
    fn test_prior_turn_failed_bash_output_is_not_pruned() {
        let output = (1..=30).map(|i| format!("error {i}")).collect::<Vec<_>>().join("\n");
        let output_with_err = format!("{output}\n\nCommand exited with code 1");
        let (call_msg, res_msg) = make_tool_turn("c1", "bash", "cargo test", &output_with_err);

        let history = vec![
            Message::user("Please test"),
            call_msg,
            res_msg,
            Message::assistant("Tests failed with exit code 1"),
            Message::user("Fix the tests"),
        ];

        let pruned = prune_historical_tool_outputs(&history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);
        let Message::User { content } = &pruned[2] else {
            panic!()
        };
        let UserContent::ToolResult(res) = &content[0] else {
            panic!()
        };
        let text = match &res.content[0] {
            ToolResultContent::Text(t) => &t.text,
            _ => panic!(),
        };
        assert!(text.contains("error 1"));
        assert!(text.contains("Command exited with code 1"));
    }

    #[test]
    fn test_prior_turn_short_bash_output_is_not_pruned() {
        let output = "line 1\nline 2\nline 3\n";
        let (call_msg, res_msg) = make_tool_turn("c1", "bash", "git status", output);

        let history = vec![
            Message::user("Status"),
            call_msg,
            res_msg,
            Message::assistant("Clean working tree"),
            Message::user("Continue"),
        ];

        let pruned = prune_historical_tool_outputs(&history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);
        let Message::User { content } = &pruned[2] else {
            panic!()
        };
        let UserContent::ToolResult(res) = &content[0] else {
            panic!()
        };
        let text = match &res.content[0] {
            ToolResultContent::Text(t) => &t.text,
            _ => panic!(),
        };
        assert_eq!(text, output);
    }

    #[test]
    fn test_prior_turn_verbose_read_output_is_pruned() {
        let output = (1..=40)
            .map(|i| format!("fn line_{i}() {{}}"))
            .collect::<Vec<_>>()
            .join("\n");
        let (call_msg, res_msg) = make_read_turn("c1", "src/lib.rs", &output);

        let history = vec![
            Message::user("Read file"),
            call_msg,
            res_msg,
            Message::assistant("Read file done"),
            Message::user("Next"),
        ];

        let pruned = prune_historical_tool_outputs(&history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);
        let Message::User { content } = &pruned[2] else {
            panic!()
        };
        let UserContent::ToolResult(res) = &content[0] else {
            panic!()
        };
        let text = match &res.content[0] {
            ToolResultContent::Text(t) => &t.text,
            _ => panic!(),
        };
        assert!(text.contains("[File 'src/lib.rs' read (40 lines,"));
        assert!(text.contains("Output pruned for historical turn."));
    }

    #[test]
    fn test_prior_turn_short_read_output_is_not_pruned() {
        let output = "fn small() {}\n";
        let (call_msg, res_msg) = make_read_turn("c1", "src/lib.rs", output);

        let history = vec![
            Message::user("Read file"),
            call_msg,
            res_msg,
            Message::assistant("Read file done"),
            Message::user("Next"),
        ];

        let pruned = prune_historical_tool_outputs(&history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);
        let Message::User { content } = &pruned[2] else {
            panic!()
        };
        let UserContent::ToolResult(res) = &content[0] else {
            panic!()
        };
        let text = match &res.content[0] {
            ToolResultContent::Text(t) => &t.text,
            _ => panic!(),
        };
        assert_eq!(text, output);
    }

    #[test]
    fn test_prior_turn_failed_read_output_is_not_pruned() {
        let output = "File not found: src/missing.rs (in working directory: /tmp)";
        let (call_msg, res_msg) = make_read_turn("c1", "src/missing.rs", output);

        let history = vec![
            Message::user("Read file"),
            call_msg,
            res_msg,
            Message::assistant("File is missing"),
            Message::user("Create it"),
        ];

        let pruned = prune_historical_tool_outputs(&history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);
        let Message::User { content } = &pruned[2] else {
            panic!()
        };
        let UserContent::ToolResult(res) = &content[0] else {
            panic!()
        };
        let text = match &res.content[0] {
            ToolResultContent::Text(t) => &t.text,
            _ => panic!(),
        };
        assert_eq!(text, output);
    }

    #[test]
    fn test_prior_turn_verbose_search_output_is_pruned() {
        let output = (1..=30)
            .map(|i| format!("crates/lib.rs:{i}:match_{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let (call_msg, res_msg) = make_search_turn("c1", "rg", "match_", &output);

        let history = vec![
            Message::user("Search matches"),
            call_msg,
            res_msg,
            Message::assistant("Found 30 matches"),
            Message::user("Continue"),
        ];

        let pruned = prune_historical_tool_outputs(&history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);
        let Message::User { content } = &pruned[2] else {
            panic!()
        };
        let UserContent::ToolResult(res) = &content[0] else {
            panic!()
        };
        let text = match &res.content[0] {
            ToolResultContent::Text(t) => &t.text,
            _ => panic!(),
        };
        assert!(text.contains("[Tool 'rg' with pattern 'match_' completed with 30 lines"));
        assert!(text.contains("Output pruned for historical turn."));
    }

    #[test]
    fn test_prior_turn_failed_search_output_is_not_pruned() {
        let output = "Search timed out after 30s";
        let (call_msg, res_msg) = make_search_turn("c1", "rg", "slow_pattern", output);

        let history = vec![
            Message::user("Search"),
            call_msg,
            res_msg,
            Message::assistant("Search timed out"),
            Message::user("Retry"),
        ];

        let pruned = prune_historical_tool_outputs(&history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);
        let Message::User { content } = &pruned[2] else {
            panic!()
        };
        let UserContent::ToolResult(res) = &content[0] else {
            panic!()
        };
        let text = match &res.content[0] {
            ToolResultContent::Text(t) => &t.text,
            _ => panic!(),
        };
        assert_eq!(text, output);
    }

    #[test]
    fn test_prior_turn_verbose_write_tool_call_is_pruned() {
        let write_body = (1..=30)
            .map(|i| format!("pub fn generated_func_{i}() {{}}"))
            .collect::<Vec<_>>()
            .join("\n");
        let (call_msg, res_msg) = make_write_turn(
            "c1",
            "src/generated.rs",
            &write_body,
            "Successfully wrote 30 lines to src/generated.rs",
        );

        let history = vec![
            Message::user("Write generated code"),
            call_msg,
            res_msg,
            Message::assistant("File written"),
            Message::user("Now test it"),
        ];

        let pruned = prune_historical_tool_outputs(&history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);
        let Message::Assistant { content, .. } = &pruned[1] else {
            panic!()
        };
        let AssistantContent::ToolCall(call) = &content[0] else {
            panic!()
        };
        let content_val = call.function.arguments.get("content").unwrap().as_str().unwrap();
        assert!(content_val.contains("[Content written (30 lines,"));
        assert_eq!(
            call.function.arguments.get("path").unwrap().as_str().unwrap(),
            "src/generated.rs"
        );
    }

    #[test]
    fn test_prior_turn_short_write_tool_call_is_not_pruned() {
        let short_body = "pub const VERSION: &str = \"1.0.0\";\n";
        let (call_msg, res_msg) = make_write_turn(
            "c1",
            "src/version.rs",
            short_body,
            "Successfully wrote 1 lines to src/version.rs",
        );

        let history = vec![
            Message::user("Set version"),
            call_msg,
            res_msg,
            Message::assistant("Version set"),
            Message::user("Next"),
        ];

        let pruned = prune_historical_tool_outputs(&history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);
        let Message::Assistant { content, .. } = &pruned[1] else {
            panic!()
        };
        let AssistantContent::ToolCall(call) = &content[0] else {
            panic!()
        };
        let content_val = call.function.arguments.get("content").unwrap().as_str().unwrap();
        assert_eq!(content_val, short_body);
    }

    #[test]
    fn test_current_turn_write_and_read_are_not_pruned() {
        let read_body = (1..=30).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        let (read_call, read_res) = make_read_turn("c1", "src/main.rs", &read_body);
        let write_body = (1..=30).map(|i| format!("code {i}")).collect::<Vec<_>>().join("\n");
        let (write_call, write_res) = make_write_turn(
            "c2",
            "src/main.rs",
            &write_body,
            "Successfully wrote 30 lines to src/main.rs",
        );

        let history = vec![
            Message::user("Active turn work"),
            read_call,
            read_res,
            write_call,
            write_res,
        ];

        let pruned = prune_historical_tool_outputs(&history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);
        assert_eq!(pruned.len(), 5);

        let Message::User { content } = &pruned[2] else {
            panic!()
        };
        let UserContent::ToolResult(res) = &content[0] else {
            panic!()
        };
        let text = match &res.content[0] {
            ToolResultContent::Text(t) => &t.text,
            _ => panic!(),
        };
        assert_eq!(text, &read_body);

        let Message::Assistant { content: a_content, .. } = &pruned[3] else {
            panic!()
        };
        let AssistantContent::ToolCall(call) = &a_content[0] else {
            panic!()
        };
        let content_val = call.function.arguments.get("content").unwrap().as_str().unwrap();
        assert_eq!(content_val, &write_body);
    }

    fn extract_tool_result_first_text(message: &Message) -> &str {
        let Message::User { content } = message else { panic!() };
        let UserContent::ToolResult(res) = &content[0] else {
            panic!()
        };
        match &res.content[0] {
            ToolResultContent::Text(t) => &t.text,
            _ => panic!(),
        }
    }

    #[test]
    fn test_multi_turn_sequence_prunes_completed_verbose_outputs_and_preserves_failures() {
        let out_verbose = (1..=25)
            .map(|i| format!("test {i} passed"))
            .collect::<Vec<_>>()
            .join("\n");
        let out_verbose_footer = format!(
            "{out_verbose}\n\n[Command completed successfully with exit code 0 (25 lines, 400B). Full log: /tmp/test.log]"
        );
        let (call1, res1) = make_tool_turn("c1", "bash", "cargo test", &out_verbose_footer);
        let out_fail = "error[E0425]: cannot find value `x` in this scope\n\nCommand exited with code 1";
        let (call2, res2) = make_tool_turn("c2", "bash", "cargo check", out_fail);
        let (call3, res3) = make_tool_turn("c3", "bash", "cargo build", &out_verbose_footer);

        let t1_history = vec![Message::user("Run test"), call1.clone(), res1.clone()];
        assert_eq!(
            prune_historical_tool_outputs(&t1_history, 1, DEFAULT_PRUNE_LINE_THRESHOLD),
            t1_history
        );

        let t2_history = vec![
            Message::user("Run test"),
            call1,
            res1,
            Message::assistant("Tests passed"),
            Message::user("Now check"),
            call2.clone(),
            res2.clone(),
        ];
        let t2_pruned = prune_historical_tool_outputs(&t2_history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);
        assert!(extract_tool_result_first_text(&t2_pruned[2]).contains("Output pruned (25 lines, 400B)"));
        assert!(extract_tool_result_first_text(&t2_pruned[6]).contains("cannot find value `x`"));

        let t3_history = vec![
            t2_pruned[0].clone(),
            t2_pruned[1].clone(),
            t2_pruned[2].clone(),
            t2_pruned[3].clone(),
            t2_pruned[4].clone(),
            call2,
            res2,
            Message::assistant("Check failed with error E0425"),
            Message::user("Now build"),
            call3,
            res3,
        ];
        let t3_pruned = prune_historical_tool_outputs(&t3_history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);
        assert!(extract_tool_result_first_text(&t3_pruned[2]).contains("Output pruned (25 lines, 400B)"));
        assert!(extract_tool_result_first_text(&t3_pruned[6]).contains("cannot find value `x`"));
        assert!(extract_tool_result_first_text(&t3_pruned[10]).contains("test 1 passed"));
    }

    #[test]
    fn test_multi_turn_mixed_tools_sequence_prunes_historical_reads_writes_and_searches() {
        let read_out = (1..=30)
            .map(|i| format!("fn f{i}() {{}}"))
            .collect::<Vec<_>>()
            .join("\n");
        let (read_call, read_res) = make_read_turn("c1", "src/models.rs", &read_out);

        let search_out = (1..=20).map(|i| format!("hit_{i}.rs")).collect::<Vec<_>>().join("\n");
        let (search_call, search_res) = make_search_turn("c2", "fd", "*.rs", &search_out);

        let write_body = (1..=25)
            .map(|i| format!("pub struct S{i};"))
            .collect::<Vec<_>>()
            .join("\n");
        let (write_call, write_res) = make_write_turn("c3", "src/types.rs", &write_body, "Wrote 25 lines");

        let bash_out = (1..=20).map(|i| format!("pass {i}")).collect::<Vec<_>>().join("\n");
        let bash_footer = format!(
            "{bash_out}\n\n[Command completed successfully with exit code 0 (20 lines, 200B). Full log: /tmp/test.log]"
        );
        let (bash_call, bash_res) = make_tool_turn("c4", "bash", "cargo test", &bash_footer);

        let history = vec![
            Message::user("Inspect codebase"),
            read_call,
            read_res,
            search_call,
            search_res,
            Message::assistant("Finished inspecting"),
            Message::user("Implement types and test"),
            write_call,
            write_res,
            bash_call,
            bash_res,
            Message::assistant("Implementation and tests complete"),
            Message::user("Now what?"),
        ];

        let pruned = prune_historical_tool_outputs(&history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);

        let read_text = extract_tool_result_first_text(&pruned[2]);
        assert!(read_text.contains("[File 'src/models.rs' read (30 lines,"));

        let search_text = extract_tool_result_first_text(&pruned[4]);
        assert!(search_text.contains("[Tool 'fd' with pattern '*.rs' completed with 20 lines"));

        let Message::Assistant {
            content: write_content, ..
        } = &pruned[7]
        else {
            panic!()
        };
        let AssistantContent::ToolCall(call) = &write_content[0] else {
            panic!()
        };
        let written_val = call.function.arguments.get("content").unwrap().as_str().unwrap();
        assert!(written_val.contains("[Content written (25 lines,"));

        let bash_text = extract_tool_result_first_text(&pruned[10]);
        assert!(bash_text.contains("Command 'cargo test' completed with exit code 0. Output pruned"));
    }

    #[test]
    fn test_prior_turn_verbose_web_fetch_output_is_pruned() {
        let output = (1..=30)
            .map(|i| format!("<p>paragraph {i}</p>"))
            .collect::<Vec<_>>()
            .join("\n");
        let (call_msg, res_msg) = make_fetch_turn("c1", "https://example.com/docs", &output);

        let history = vec![
            Message::user("Fetch docs"),
            call_msg,
            res_msg,
            Message::assistant("Docs fetched"),
            Message::user("Next"),
        ];

        let pruned = prune_historical_tool_outputs(&history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);
        let text = extract_tool_result_first_text(&pruned[2]);
        assert!(text.contains("[URL 'https://example.com/docs' fetched (30 lines,"));
        assert!(text.contains("Output pruned for historical turn."));
    }

    #[test]
    fn test_prior_turn_failed_web_fetch_output_is_not_pruned() {
        let output = "Failed to fetch https://example.com: Connection refused";
        let (call_msg, res_msg) = make_fetch_turn("c1", "https://example.com", output);

        let history = vec![
            Message::user("Fetch"),
            call_msg,
            res_msg,
            Message::assistant("Failed to fetch"),
            Message::user("Retry"),
        ];

        let pruned = prune_historical_tool_outputs(&history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);
        let text = extract_tool_result_first_text(&pruned[2]);
        assert_eq!(text, output);
    }

    #[test]
    fn test_prior_turn_verbose_web_search_output_is_pruned() {
        let output = (1..=25)
            .map(|i| format!("{i}. Result title and snippet"))
            .collect::<Vec<_>>()
            .join("\n");
        let (call_msg, res_msg) = make_search_turn("c1", "web_search", "rust tokio", &output);

        let history = vec![
            Message::user("Search rust"),
            call_msg,
            res_msg,
            Message::assistant("Search results found"),
            Message::user("Next"),
        ];

        let pruned = prune_historical_tool_outputs(&history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);
        let text = extract_tool_result_first_text(&pruned[2]);
        assert!(text.contains("[Web search for 'rust tokio' completed with 25 lines"));
    }

    #[test]
    fn test_prior_turn_verbose_mcp_output_is_pruned() {
        let output = (1..=40)
            .map(|i| format!("{{\"record\": {i}}}"))
            .collect::<Vec<_>>()
            .join("\n");
        let (call_msg, res_msg) = make_mcp_turn("c1", "mcp__github__list_issues", &output);

        let history = vec![
            Message::user("List issues"),
            call_msg,
            res_msg,
            Message::assistant("Issues listed"),
            Message::user("Next"),
        ];

        let pruned = prune_historical_tool_outputs(&history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);
        let text = extract_tool_result_first_text(&pruned[2]);
        assert!(text.contains("[Tool 'mcp__github__list_issues' completed with 40 lines"));
    }

    #[test]
    fn test_prior_turn_verbose_edit_tool_call_is_pruned() {
        let old_text = (1..=30).map(|i| format!("old line {i}")).collect::<Vec<_>>().join("\n");
        let new_text = (1..=30).map(|i| format!("new line {i}")).collect::<Vec<_>>().join("\n");
        let (call_msg, res_msg) = make_edit_turn(
            "c1",
            "src/file.rs",
            vec![(&old_text, &new_text)],
            "Successfully applied 1 replacement(s) to src/file.rs",
        );

        let history = vec![
            Message::user("Edit file"),
            call_msg,
            res_msg,
            Message::assistant("Edit applied"),
            Message::user("Next"),
        ];

        let pruned = prune_historical_tool_outputs(&history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);
        let Message::Assistant { content, .. } = &pruned[1] else {
            panic!()
        };
        let AssistantContent::ToolCall(call) = &content[0] else {
            panic!()
        };
        let edits = call.function.arguments.get("edits").unwrap().as_array().unwrap();
        let old_val = edits[0].get("oldText").unwrap().as_str().unwrap();
        let new_val = edits[0].get("newText").unwrap().as_str().unwrap();
        assert!(old_val.contains("[Old text (30 lines,"));
        assert!(new_val.contains("[New text (30 lines,"));
    }

    #[test]
    fn test_prior_turn_short_edit_tool_call_is_not_pruned() {
        let (call_msg, res_msg) = make_edit_turn(
            "c1",
            "src/file.rs",
            vec![("let x = 1;", "let x = 2;")],
            "Successfully applied 1 replacement(s) to src/file.rs",
        );

        let history = vec![
            Message::user("Edit file"),
            call_msg,
            res_msg,
            Message::assistant("Edit applied"),
            Message::user("Next"),
        ];

        let pruned = prune_historical_tool_outputs(&history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);
        let Message::Assistant { content, .. } = &pruned[1] else {
            panic!()
        };
        let AssistantContent::ToolCall(call) = &content[0] else {
            panic!()
        };
        let edits = call.function.arguments.get("edits").unwrap().as_array().unwrap();
        assert_eq!(edits[0].get("oldText").unwrap().as_str().unwrap(), "let x = 1;");
        assert_eq!(edits[0].get("newText").unwrap().as_str().unwrap(), "let x = 2;");
    }
}
