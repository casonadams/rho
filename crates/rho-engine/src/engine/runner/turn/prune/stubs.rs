use std::collections::HashMap;

use rho_harness_core::model::{AssistantContent, ChatMessage, ImageContent};

use crate::tools::bash::footer::BashFooter;

enum BashExitStatus {
    Success,
    Failed(String),
}

struct BashPruneDetails {
    lines: usize,
    size_str: String,
    log_path: Option<String>,
    exit_status: BashExitStatus,
}

fn parse_accumulator_lines(text: &str) -> Option<usize> {
    let start = text.rfind("[lines ").or_else(|| text.rfind("[Showing lines "))?;
    let of_pos = text[start..].find(" of ")? + start + 4;
    let end = text[of_pos..].find([' ', ';'])?;
    text[of_pos..of_pos + end].parse().ok()
}

fn extract_bash_log_path(text: &str) -> Option<String> {
    const MARKERS: [&str; 4] = ["; log: ", "; full log: ", "Full log:", "Full output:"];
    let (start, marker) = MARKERS
        .iter()
        .find_map(|marker| text.rfind(marker).map(|start| (start, marker)))?;
    let path = text[start + marker.len()..].split_whitespace().next()?;
    Some(path.trim_matches(']').to_string())
}

fn extract_bash_exit_code(text: &str) -> Option<String> {
    if let Some(pos) = text.rfind("[exit ") {
        let code = text[pos + "[exit ".len()..].split(']').next()?;
        if !code.is_empty() && code.chars().all(|c| c.is_ascii_digit() || c == '-') {
            return Some(code.to_string());
        }
    }
    if let Some(pos) = text.rfind("Command exited with code ") {
        let after = text[pos + "Command exited with code ".len()..].trim();
        let code = after
            .split_whitespace()
            .next()?
            .trim_matches(|c: char| !c.is_ascii_digit() && c != '-');
        if !code.is_empty() {
            return Some(code.to_string());
        }
    }
    if text.contains("[timeout ") || text.contains("Command timed out after ") {
        return Some("timeout".to_string());
    }
    None
}

fn parse_bash_footer_details(text: &str) -> Option<BashPruneDetails> {
    let footer = BashFooter::parse(text)?;
    Some(BashPruneDetails {
        lines: footer.lines,
        size_str: footer.size,
        log_path: footer.log_path,
        exit_status: BashExitStatus::Success,
    })
}

fn check_prunable_bash_output(text: &str, line_threshold: usize) -> Option<BashPruneDetails> {
    if let Some(details) = parse_bash_footer_details(text) {
        if details.lines > line_threshold {
            return Some(details);
        }
        return None;
    }

    let line_count = parse_accumulator_lines(text).unwrap_or_else(|| text.lines().count());
    if line_count > line_threshold {
        let exit_status = if let Some(code) = extract_bash_exit_code(text) {
            BashExitStatus::Failed(code)
        } else {
            BashExitStatus::Success
        };
        let log_path = extract_bash_log_path(text);
        Some(BashPruneDetails {
            lines: line_count,
            size_str: crate::tools::truncate::format_size(text.len()),
            log_path,
            exit_status,
        })
    } else {
        None
    }
}

#[derive(Debug, Clone)]
pub(super) struct ToolCallMeta {
    pub(super) name: String,
    pub(super) target: String,
}

fn extract_tool_call_target(name: &str, args: &serde_json::Value) -> String {
    match name {
        "bash" => args
            .get("command")
            .and_then(|v| v.as_str())
            .unwrap_or("bash")
            .to_string(),
        "read" | "read_file" | "write" | "write_file" | "edit" | "edit_file" => {
            args.get("path").and_then(|v| v.as_str()).unwrap_or("").to_string()
        }
        "rg" | "grep" | "fd" | "find" | "glob" => args
            .get("pattern")
            .and_then(|v| v.as_str())
            .or_else(|| args.get("query").and_then(|v| v.as_str()))
            .unwrap_or("")
            .to_string(),
        "web_fetch" | "webfetch" => args.get("url").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        "web_search" | "websearch" => args
            .get("query")
            .or_else(|| args.get("pattern"))
            .or_else(|| args.get("q"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        _ => args
            .get("path")
            .or_else(|| args.get("target"))
            .or_else(|| args.get("url"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
    }
}

pub(super) fn collect_tool_calls(messages: &[ChatMessage]) -> HashMap<String, ToolCallMeta> {
    let mut map = HashMap::new();
    for msg in messages {
        if let ChatMessage::Assistant { content, .. } = msg {
            for item in content {
                if let AssistantContent::ToolCall(call) = item {
                    let name = call.function.name.to_ascii_lowercase();
                    let target = extract_tool_call_target(&name, &call.function.arguments);
                    map.insert(call.id.to_string(), ToolCallMeta { name, target });
                }
            }
        }
    }
    map
}

fn format_pruned_stub(cmd: &str, details: &BashPruneDetails) -> String {
    let status = match &details.exit_status {
        BashExitStatus::Success => "exit 0".to_string(),
        BashExitStatus::Failed(code) if code == "timeout" => code.clone(),
        BashExitStatus::Failed(code) => format!("exit {code}"),
    };
    let log = details
        .log_path
        .as_ref()
        .map(|path| format!("; log: {path}"))
        .unwrap_or_default();
    format!(
        "{}{status}, pruned {} lines, {}{log}]",
        stub_head("bash", cmd),
        details.lines,
        details.size_str
    )
}

fn is_read_error(text: &str) -> bool {
    text.starts_with("Error")
        || text.starts_with("File not found")
        || text.starts_with("Failed to read")
        || text.starts_with("Empty file path")
        || text.starts_with("File contains invalid UTF-8")
        || (text.starts_with("Offset ") && text.contains("is beyond end of file"))
}

fn is_search_error(text: &str) -> bool {
    text.starts_with("Error") || text.starts_with("Search timed out") || text.starts_with("Failed ")
}

fn is_fetch_error(text: &str) -> bool {
    text.starts_with("Error") || text.starts_with("Empty URL") || text.starts_with("Failed ")
}

pub(super) fn prune_bash_result(text: &str, meta: Option<&ToolCallMeta>, line_threshold: usize) -> Option<String> {
    let details = check_prunable_bash_output(text, line_threshold)?;
    let cmd = meta.map(|m| m.target.as_str()).unwrap_or("bash");
    Some(format_pruned_stub(cmd, &details))
}

fn extract_mime_from_read_text(text: &str) -> Option<&str> {
    let start = text.find("Read image file [")? + "Read image file [".len();
    let end = text[start..].find(']')?;
    let mime = &text[start..start + end];
    if mime.is_empty() { None } else { Some(mime) }
}

pub(super) fn prune_image_tool_result(
    tool_name: &str,
    text: &str,
    meta: Option<&ToolCallMeta>,
    image: &ImageContent,
) -> String {
    let target = meta.map(|m| m.target.as_str()).unwrap_or("");
    let media_type = image.media_type.as_deref().unwrap_or("unknown");
    let mime = if media_type != "unknown" {
        Some(media_type)
    } else {
        extract_mime_from_read_text(text)
    };
    let mime_suffix = mime.map(|m| format!(" ({m})")).unwrap_or_default();
    let label = if tool_name.is_empty() || tool_name == "read_file" {
        "read"
    } else {
        tool_name
    };
    format!("{}image{mime_suffix}, pruned]", stub_head(label, target))
}

/// `[label 'target': ` or `[label: `, the start of every pruned-result stub.
pub(super) fn stub_head(label: &str, target: &str) -> String {
    if target.is_empty() {
        format!("[{label}: ")
    } else {
        format!("[{label} '{target}': ")
    }
}

fn counted_stub(label: &str, target: &str, line_count: usize, size_str: &str) -> String {
    format!("{}pruned {line_count} lines, {size_str}]", stub_head(label, target))
}

fn spill_artifact_path(text: &str) -> Option<&str> {
    let (_, notice) = crate::tools::truncate::split_artifact_notice(text);
    let after = &notice?[notice?.find(" saved to ")? + " saved to ".len()..];
    Some(&after[..after.find(". Use '")?])
}

fn with_artifact_path(stub: String, text: &str) -> String {
    match spill_artifact_path(text) {
        Some(path) => format!("{}; full: {path}]", stub.trim_end_matches(']')),
        None => stub,
    }
}

pub(super) fn prune_read_result(text: &str, meta: Option<&ToolCallMeta>, line_threshold: usize) -> Option<String> {
    if is_read_error(text) {
        return None;
    }
    let line_count = text.lines().count();
    if line_count <= line_threshold {
        return None;
    }
    let size_str = crate::tools::truncate::format_size(text.len());
    let target = meta.map(|m| m.target.as_str()).unwrap_or("");
    let stub = counted_stub("read", target, line_count, &size_str);
    Some(with_artifact_path(stub, text))
}

pub(super) fn prune_search_result(
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
    Some(counted_stub(tool_name, target, line_count, &size_str))
}

fn prune_fetch_result(text: &str, target: &str, line_count: usize, size_str: &str) -> Option<String> {
    if is_fetch_error(text) {
        return None;
    }
    Some(counted_stub("web_fetch", target, line_count, size_str))
}

fn prune_web_search_result(text: &str, target: &str, line_count: usize, size_str: &str) -> Option<String> {
    if is_search_error(text) {
        return None;
    }
    Some(counted_stub("web_search", target, line_count, size_str))
}

pub(super) fn prune_web_result(
    tool_name: &str,
    text: &str,
    meta: Option<&ToolCallMeta>,
    line_threshold: usize,
) -> Option<String> {
    let line_count = text.lines().count();
    if line_count <= line_threshold {
        return None;
    }
    let size_str = crate::tools::truncate::format_size(text.len());
    let target = meta.map_or("", |m| m.target.as_str());
    match tool_name {
        "web_fetch" | "webfetch" => prune_fetch_result(text, target, line_count, &size_str),
        "web_search" | "websearch" => prune_web_search_result(text, target, line_count, &size_str),
        _ => None,
    }
}

pub(super) fn prune_mcp_result(tool_name: &str, text: &str, line_threshold: usize) -> Option<String> {
    if text.starts_with("Error") || text.starts_with("Failed ") {
        return None;
    }
    let line_count = text.lines().count();
    if line_count <= line_threshold {
        return None;
    }
    let size_str = crate::tools::truncate::format_size(text.len());
    Some(counted_stub(tool_name, "", line_count, &size_str))
}
