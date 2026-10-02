use std::collections::{HashMap, HashSet};

use rho_harness_core::tokens::cut_point::is_user_turn_start;
use rig::completion::message::MimeType;
use rig::message::{AssistantContent, Message, ToolCall, ToolResultContent, UserContent};

pub const DEFAULT_PRUNE_LINE_THRESHOLD: usize = 15;
pub const DEFAULT_PROTECT_RECENT_TOKENS: usize = 30_000;
pub const DEFAULT_PRUNE_MINIMUM_SAVINGS: usize = 20_000;
pub const DEFAULT_MIN_PRUNE_TOKENS: usize = 50;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrunePolicy {
    pub volatile_turns: usize,
    pub line_threshold: usize,
    pub protect_recent_tokens: usize,
    pub prune_minimum_tokens: usize,
    pub min_prune_tokens: usize,
    pub model: Option<String>,
}

impl Default for PrunePolicy {
    fn default() -> Self {
        Self::cache_preserving()
    }
}

impl PrunePolicy {
    pub const fn unconstrained(volatile_turns: usize, line_threshold: usize) -> Self {
        Self {
            volatile_turns,
            line_threshold,
            protect_recent_tokens: 0,
            prune_minimum_tokens: 0,
            min_prune_tokens: 0,
            model: None,
        }
    }

    pub const fn cache_preserving() -> Self {
        Self {
            volatile_turns: 1,
            line_threshold: DEFAULT_PRUNE_LINE_THRESHOLD,
            protect_recent_tokens: DEFAULT_PROTECT_RECENT_TOKENS,
            prune_minimum_tokens: DEFAULT_PRUNE_MINIMUM_SAVINGS,
            min_prune_tokens: DEFAULT_MIN_PRUNE_TOKENS,
            model: None,
        }
    }

    pub fn for_context_window(context_window: usize) -> Self {
        let savings_floor = (context_window / 20).clamp(3_000, DEFAULT_PRUNE_MINIMUM_SAVINGS);
        let protect_recent = (context_window * 3 / 20).clamp(10_000, DEFAULT_PROTECT_RECENT_TOKENS);
        Self {
            volatile_turns: 1,
            line_threshold: DEFAULT_PRUNE_LINE_THRESHOLD,
            protect_recent_tokens: protect_recent,
            prune_minimum_tokens: savings_floor,
            min_prune_tokens: DEFAULT_MIN_PRUNE_TOKENS,
            model: None,
        }
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }
}

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
    let start = text.rfind("[Showing lines ")?;
    let of_pos = text[start..].find(" of ")? + start + 4;
    let end = text[of_pos..].find(' ')?;
    text[of_pos..of_pos + end].parse().ok()
}

fn extract_bash_log_path(text: &str) -> Option<String> {
    if let Some(path_start) = text.rfind("Full log:") {
        let after = text[path_start + "Full log:".len()..].trim();
        let path = after.split_whitespace().next()?.trim_matches(']');
        return Some(path.to_string());
    }
    if let Some(path_start) = text.rfind("Full output:") {
        let after = text[path_start + "Full output:".len()..].trim();
        let path = after.split_whitespace().next()?.trim_matches(']');
        return Some(path.to_string());
    }
    None
}

fn extract_bash_exit_code(text: &str) -> Option<String> {
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
    if text.contains("Command timed out after ") {
        return Some("timeout".to_string());
    }
    None
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
    let log_path = extract_bash_log_path(inner);

    Some(BashPruneDetails {
        lines,
        size_str: size_part.to_string(),
        log_path,
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
struct ToolCallMeta {
    name: String,
    target: String,
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

fn collect_tool_calls(messages: &[Message]) -> HashMap<String, ToolCallMeta> {
    let mut map = HashMap::new();
    for msg in messages {
        if let Message::Assistant { content, .. } = msg {
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
    match (&details.exit_status, &details.log_path) {
        (BashExitStatus::Success, Some(path)) => format!(
            "[Command '{cmd}' completed with exit code 0. Output pruned ({} lines, {}). Full log: {path}]",
            details.lines, details.size_str
        ),
        (BashExitStatus::Success, None) => format!(
            "[Command '{cmd}' completed with exit code 0. Output pruned ({} lines, {}).]",
            details.lines, details.size_str
        ),
        (BashExitStatus::Failed(code), Some(path)) => format!(
            "[Command '{cmd}' failed with exit code {code} ({} lines, {}). Full log: {path}]",
            details.lines, details.size_str
        ),
        (BashExitStatus::Failed(code), None) => format!(
            "[Command '{cmd}' failed with exit code {code} ({} lines, {}).]",
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

fn extract_mime_from_read_text(text: &str) -> Option<&str> {
    let start = text.find("Read image file [")? + "Read image file [".len();
    let end = text[start..].find(']')?;
    let mime = &text[start..start + end];
    if mime.is_empty() { None } else { Some(mime) }
}

fn prune_image_tool_result(
    tool_name: &str,
    text: &str,
    meta: Option<&ToolCallMeta>,
    image: &rig::completion::message::Image,
) -> String {
    let target = meta.map(|m| m.target.as_str()).unwrap_or("");
    let media_type = image.media_type.as_ref().map_or("unknown", MimeType::to_mime_type);
    let mime = if media_type != "unknown" {
        Some(media_type)
    } else {
        extract_mime_from_read_text(text)
    };
    let mime_suffix = mime.map(|m| format!(" ({m})")).unwrap_or_default();

    if tool_name == "read" || tool_name == "read_file" || tool_name.is_empty() {
        if target.is_empty() {
            format!("[Image{mime_suffix} read. Image content pruned for historical turn.]")
        } else {
            format!("[Image '{target}'{mime_suffix} read. Image content pruned for historical turn.]")
        }
    } else if target.is_empty() {
        format!("[Tool '{tool_name}' returned image{mime_suffix}. Image content pruned for historical turn.]")
    } else {
        format!(
            "[Tool '{tool_name}' for '{target}' returned image{mime_suffix}. Image content pruned for historical turn.]"
        )
    }
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

fn prune_fetch_result(text: &str, target: &str, line_count: usize, size_str: &str) -> Option<String> {
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
}

fn prune_web_search_result(text: &str, target: &str, line_count: usize, size_str: &str) -> Option<String> {
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
}

fn prune_web_result(tool_name: &str, text: &str, meta: Option<&ToolCallMeta>, line_threshold: usize) -> Option<String> {
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

fn is_read_tool(name: &str) -> bool {
    matches!(name, "read" | "read_file")
}

fn is_write_or_edit_tool(name: &str) -> bool {
    matches!(name, "write" | "write_file" | "edit" | "edit_file")
}

fn extract_file_path_arg(args: &serde_json::Value) -> Option<&str> {
    args.get("path")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
}

fn process_tool_call_path<'a>(call: &'a ToolCall, seen_paths: &mut HashSet<&'a str>, superseded: &mut HashSet<String>) {
    let Some(path) = extract_file_path_arg(&call.function.arguments) else {
        return;
    };
    let name = call.function.name.as_str();
    if is_read_tool(name) {
        if !seen_paths.insert(path) {
            superseded.insert(call.id.to_string());
        }
    } else if is_write_or_edit_tool(name) {
        seen_paths.insert(path);
    }
}

pub fn collect_superseded_tool_call_ids(messages: &[Message]) -> HashSet<String> {
    let mut seen_paths: HashSet<&str> = HashSet::new();
    let mut superseded = HashSet::new();
    for msg in messages.iter().rev() {
        let Message::Assistant { content, .. } = msg else {
            continue;
        };
        for item in content.iter().rev() {
            if let AssistantContent::ToolCall(call) = item {
                process_tool_call_path(call, &mut seen_paths, &mut superseded);
            }
        }
    }
    superseded
}

pub fn find_recent_tokens_cutoff(messages: &[Message], protect_tokens: usize, model: &str) -> usize {
    if protect_tokens == 0 || messages.is_empty() {
        return messages.len();
    }
    let mut accumulated = 0usize;
    for (idx, msg) in messages.iter().enumerate().rev() {
        accumulated = accumulated.saturating_add(rho_harness_core::tokens::estimate_message_tokens(msg, model));
        if accumulated >= protect_tokens {
            return idx;
        }
    }
    0
}

struct PruneContext<'a> {
    tool_calls: &'a HashMap<String, ToolCallMeta>,
    superseded_call_ids: &'a HashSet<String>,
    line_threshold: usize,
    min_prune_tokens: usize,
    model: &'a str,
}

fn dispatch_tool_prune_stub(
    tool_name: &str,
    text: &str,
    meta: Option<&ToolCallMeta>,
    line_threshold: usize,
) -> Option<String> {
    match tool_name {
        "bash" => prune_bash_result(text, meta, line_threshold),
        "read" | "read_file" => prune_read_result(text, meta, line_threshold),
        "rg" | "grep" | "fd" | "find" | "glob" => prune_search_result(tool_name, text, meta, line_threshold),
        name if name.starts_with("web") => prune_web_result(name, text, meta, line_threshold),
        name if name.contains("__") => prune_mcp_result(name, text, line_threshold),
        _ => None,
    }
}

fn extract_tool_result_text(contents: &[ToolResultContent]) -> String {
    contents
        .iter()
        .filter_map(|c| match c {
            ToolResultContent::Text(t) => Some(t.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn resolve_tool_prune_stub(
    tool_name: &str,
    text: &str,
    meta: Option<&ToolCallMeta>,
    image_block: Option<&rig::completion::message::Image>,
    is_superseded: bool,
    line_threshold: usize,
) -> Option<String> {
    if is_superseded {
        let target = meta.map_or("", |m| m.target.as_str());
        if target.is_empty() {
            Some("[File read superseded by a later operation]".to_string())
        } else {
            Some(format!("[File read superseded by a later operation on {target}]"))
        }
    } else if let Some(img) = image_block {
        Some(prune_image_tool_result(tool_name, text, meta, img))
    } else {
        dispatch_tool_prune_stub(tool_name, text, meta, line_threshold)
    }
}

fn prune_tool_result_item(item: &UserContent, ctx: &PruneContext<'_>) -> Option<UserContent> {
    let UserContent::ToolResult(res) = item else {
        return None;
    };
    let meta = ctx.tool_calls.get(&res.call.to_string());
    let tool_name = if !res.name.is_empty() {
        res.name.to_ascii_lowercase()
    } else {
        meta.map_or(String::new(), |m| m.name.clone())
    };

    if tool_name == "skill" || tool_name.starts_with("skill_") {
        return None;
    }

    let is_superseded = ctx.superseded_call_ids.contains(&res.call.to_string());
    let text = extract_tool_result_text(&res.content);

    if ctx.min_prune_tokens > 0 && !is_superseded {
        let tokens = rho_harness_core::tokens::estimate_text_tokens(&text, ctx.model);
        if tokens < ctx.min_prune_tokens {
            return None;
        }
    }

    let image_block = res.content.iter().find_map(|c| match c {
        ToolResultContent::Image(img) => Some(img),
        _ => None,
    });

    let stub = resolve_tool_prune_stub(&tool_name, &text, meta, image_block, is_superseded, ctx.line_threshold)?;

    Some(UserContent::ToolResult(rig::message::ToolResult {
        call: res.call.clone(),
        provider: res.provider.clone(),
        name: res.name.clone(),
        content: vec![ToolResultContent::text(stub)],
    }))
}

fn prune_user_message_content(content: &[UserContent], ctx: &PruneContext<'_>) -> Option<Vec<UserContent>> {
    let mut modified = false;
    let new_content: Vec<UserContent> = content
        .iter()
        .map(|item| match prune_tool_result_item(item, ctx) {
            Some(pruned) => {
                modified = true;
                pruned
            }
            None => item.clone(),
        })
        .collect();

    if modified { Some(new_content) } else { None }
}

pub fn prune_historical_tool_outputs_with_policy(messages: &[Message], policy: &PrunePolicy) -> Vec<Message> {
    if messages.is_empty() {
        return Vec::new();
    }

    let model = policy.model.as_deref().unwrap_or("gpt-4");
    let cutoff_idx = find_turn_boundary_cutoff(messages, policy.volatile_turns);
    let recent_cutoff_idx = find_recent_tokens_cutoff(messages, policy.protect_recent_tokens, model);
    let effective_cutoff = cutoff_idx.min(recent_cutoff_idx);
    if effective_cutoff == 0 {
        return messages.to_vec();
    }

    let tool_calls = collect_tool_calls(messages);
    let superseded_ids = collect_superseded_tool_call_ids(messages);
    let ctx = PruneContext {
        tool_calls: &tool_calls,
        superseded_call_ids: &superseded_ids,
        line_threshold: policy.line_threshold,
        min_prune_tokens: policy.min_prune_tokens,
        model,
    };

    let mut total_savings = 0usize;
    let pruned_messages: Vec<Message> = messages
        .iter()
        .enumerate()
        .map(|(idx, msg)| {
            if idx >= effective_cutoff {
                return msg.clone();
            }
            match msg {
                Message::User { content } => match prune_user_message_content(content, &ctx) {
                    Some(new_content) => {
                        let original_tokens = rho_harness_core::tokens::estimate_message_tokens(msg, model);
                        let new_msg = Message::User { content: new_content };
                        let new_tokens = rho_harness_core::tokens::estimate_message_tokens(&new_msg, model);
                        total_savings = total_savings.saturating_add(original_tokens.saturating_sub(new_tokens));
                        new_msg
                    }
                    None => msg.clone(),
                },
                _ => msg.clone(),
            }
        })
        .collect();

    if policy.prune_minimum_tokens > 0 && total_savings < policy.prune_minimum_tokens {
        return messages.to_vec();
    }

    pruned_messages
}

pub fn prune_historical_tool_outputs(
    messages: &[Message],
    volatile_turns: usize,
    line_threshold: usize,
) -> Vec<Message> {
    prune_historical_tool_outputs_with_policy(messages, &PrunePolicy::unconstrained(volatile_turns, line_threshold))
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

    fn make_image_read_turn(
        cid: &str,
        path: &str,
        tool_output: &str,
        media_type: Option<rig::completion::message::ImageMediaType>,
    ) -> (Message, Message) {
        let call = ToolCall::new(
            ToolCallId::new_or_mint(cid),
            ToolFunction::new("read".to_string(), serde_json::json!({ "path": path })),
        );
        let res = rig::message::ToolResult {
            call: ToolCallId::new_or_mint(cid),
            provider: None,
            name: "read".to_string(),
            content: vec![
                ToolResultContent::Text(Text::new(tool_output)),
                ToolResultContent::image_base64("iVBORw0KGgoAAAANSUhEUgA=", media_type, None),
            ],
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
    fn test_historical_failed_bash_output_is_pruned() {
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
        assert!(text.contains("[Command 'cargo test' failed with exit code 1 (32 lines,"));
    }

    #[test]
    fn test_recent_failed_bash_output_within_protection_window_is_not_pruned() {
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

        let policy = PrunePolicy::cache_preserving();
        let pruned = prune_historical_tool_outputs_with_policy(&history, &policy);
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
    fn test_historical_failed_bash_output_with_log_path_pruned() {
        let err_body = (1..=30)
            .map(|i| format!("compiler error {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let output = format!(
            "{err_body}\n\n[Showing lines 1-500 of 1200 (50.0KB limit). Full output: /tmp/rho-bash-123.log]\n\nCommand exited with code 1"
        );
        let (call_msg, res_msg) = make_tool_turn("c1", "bash", "cargo build", &output);

        let large_padding = "word ".repeat(50_000);
        let history = vec![
            Message::user("Build the project"),
            call_msg,
            res_msg,
            Message::assistant("Build failed with exit code 1"),
            Message::user(large_padding),
            Message::assistant("Acknowledge large output"),
            Message::user("Continue work"),
        ];

        let policy = PrunePolicy {
            prune_minimum_tokens: 0,
            ..PrunePolicy::cache_preserving()
        };
        let pruned = prune_historical_tool_outputs_with_policy(&history, &policy);
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
        assert!(text.contains("[Command 'cargo build' failed with exit code 1 (1200 lines,"));
        assert!(text.contains("Full log: /tmp/rho-bash-123.log]"));
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
    fn test_prior_turn_write_tool_call_is_never_pruned() {
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
        assert_eq!(content_val, write_body);
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
        assert_eq!(written_val, write_body);

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
    fn test_prior_turn_edit_tool_call_is_never_pruned() {
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
        assert_eq!(old_val, old_text);
        assert_eq!(new_val, new_text);
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

    #[test]
    fn test_prune_image_tool_result_in_historical_turn() {
        let (read_call, read_res) = make_image_read_turn(
            "c1",
            "screenshot.png",
            "Read image file [image/png]\n[Image: 800x600]",
            Some(rig::completion::message::ImageMediaType::PNG),
        );

        let history = vec![
            Message::user("Inspect screenshot"),
            read_call,
            read_res,
            Message::assistant("The screenshot shows a login form."),
            Message::user("Now fix the login form"),
        ];

        let pruned = prune_historical_tool_outputs(&history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);
        let Message::User { content } = &pruned[2] else {
            panic!()
        };
        let UserContent::ToolResult(res) = &content[0] else {
            panic!()
        };

        assert_eq!(res.content.len(), 1);
        let ToolResultContent::Text(text) = &res.content[0] else {
            panic!("Expected text block")
        };
        assert_eq!(
            text.text,
            "[Image 'screenshot.png' (image/png) read. Image content pruned for historical turn.]"
        );
    }

    #[test]
    fn test_preserve_image_tool_result_in_volatile_turn() {
        let (read_call, read_res) = make_image_read_turn(
            "c1",
            "screenshot.png",
            "Read image file [image/png]\n[Image: 800x600]",
            Some(rig::completion::message::ImageMediaType::PNG),
        );

        let history = vec![Message::user("Inspect screenshot"), read_call, read_res];

        let pruned = prune_historical_tool_outputs(&history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);
        let Message::User { content } = &pruned[2] else {
            panic!()
        };
        let UserContent::ToolResult(res) = &content[0] else {
            panic!()
        };

        assert_eq!(res.content.len(), 2);
        assert!(matches!(res.content[0], ToolResultContent::Text(_)));
        assert!(matches!(res.content[1], ToolResultContent::Image(_)));
    }

    #[test]
    fn test_image_stub_formatting_variants() {
        let (call1, res1) = make_image_read_turn(
            "c1",
            "assets/logo.png",
            "Read image file [image/png]",
            Some(rig::completion::message::ImageMediaType::PNG),
        );
        let (call2, res2) = make_image_read_turn("c2", "assets/photo.jpg", "Read image file [image/jpeg]", None);

        let custom_call = ToolCall::new(
            ToolCallId::new_or_mint("c3"),
            ToolFunction::new("screenshot".to_string(), serde_json::json!({ "target": "window" })),
        );
        let custom_res = rig::message::ToolResult {
            call: ToolCallId::new_or_mint("c3"),
            provider: None,
            name: "screenshot".to_string(),
            content: vec![
                ToolResultContent::Text(Text::new("Captured screenshot")),
                ToolResultContent::image_base64(
                    "iVBORw0KGgoAAAANSUhEUgA=",
                    Some(rig::completion::message::ImageMediaType::PNG),
                    None,
                ),
            ],
        };
        let custom_call_msg = Message::Assistant {
            id: None,
            content: vec![AssistantContent::ToolCall(custom_call)],
        };
        let custom_res_msg = Message::User {
            content: vec![UserContent::ToolResult(custom_res)],
        };

        let history = vec![
            Message::user("Inspect images"),
            call1,
            res1,
            call2,
            res2,
            custom_call_msg,
            custom_res_msg,
            Message::assistant("All images inspected"),
            Message::user("Next step"),
        ];

        let pruned = prune_historical_tool_outputs(&history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);

        let extract_text = |idx: usize| -> String {
            let Message::User { content } = &pruned[idx] else {
                panic!()
            };
            let UserContent::ToolResult(res) = &content[0] else {
                panic!()
            };
            match &res.content[0] {
                ToolResultContent::Text(t) => t.text.clone(),
                _ => panic!(),
            }
        };

        assert_eq!(
            extract_text(2),
            "[Image 'assets/logo.png' (image/png) read. Image content pruned for historical turn.]"
        );
        assert_eq!(
            extract_text(4),
            "[Image 'assets/photo.jpg' (image/jpeg) read. Image content pruned for historical turn.]"
        );
        assert_eq!(
            extract_text(6),
            "[Tool 'screenshot' for 'window' returned image (image/png). Image content pruned for historical turn.]"
        );
    }

    #[test]
    fn test_protect_recent_tokens_prevents_pruning() {
        let (call, res) = make_tool_turn("c1", "bash", "cargo test", &"verbose test output\n".repeat(30));
        let history = vec![
            Message::user("Run test"),
            call,
            res,
            Message::assistant("Done test"),
            Message::user("Next step"),
        ];

        let policy_protected = PrunePolicy {
            volatile_turns: 1,
            line_threshold: DEFAULT_PRUNE_LINE_THRESHOLD,
            protect_recent_tokens: 30_000,
            prune_minimum_tokens: 0,
            min_prune_tokens: 0,
            model: None,
        };
        let pruned = prune_historical_tool_outputs_with_policy(&history, &policy_protected);
        assert_eq!(pruned, history);

        let policy_unprotected = PrunePolicy {
            volatile_turns: 1,
            line_threshold: DEFAULT_PRUNE_LINE_THRESHOLD,
            protect_recent_tokens: 5,
            prune_minimum_tokens: 0,
            min_prune_tokens: 0,
            model: None,
        };
        let pruned_unprotected = prune_historical_tool_outputs_with_policy(&history, &policy_unprotected);
        assert_ne!(pruned_unprotected, history);
        let Message::User { content } = &pruned_unprotected[2] else {
            panic!()
        };
        let UserContent::ToolResult(r) = &content[0] else {
            panic!()
        };
        let ToolResultContent::Text(t) = &r.content[0] else {
            panic!()
        };
        assert!(t.text.contains("completed with exit code 0"));
    }

    #[test]
    fn test_prune_minimum_tokens_savings_floor() {
        let (call, res) = make_tool_turn("c1", "bash", "cargo test", &"verbose test output\n".repeat(30));
        let history = vec![
            Message::user("Run test"),
            call,
            res,
            Message::assistant("Done test"),
            Message::user("Next step"),
        ];

        let high_savings_floor = PrunePolicy {
            volatile_turns: 1,
            line_threshold: DEFAULT_PRUNE_LINE_THRESHOLD,
            protect_recent_tokens: 0,
            prune_minimum_tokens: 50_000,
            min_prune_tokens: 0,
            model: None,
        };
        let pruned_skipped = prune_historical_tool_outputs_with_policy(&history, &high_savings_floor);
        assert_eq!(pruned_skipped, history);

        let low_savings_floor = PrunePolicy {
            volatile_turns: 1,
            line_threshold: DEFAULT_PRUNE_LINE_THRESHOLD,
            protect_recent_tokens: 0,
            prune_minimum_tokens: 10,
            min_prune_tokens: 0,
            model: None,
        };
        let pruned_executed = prune_historical_tool_outputs_with_policy(&history, &low_savings_floor);
        assert_ne!(pruned_executed, history);
    }

    #[test]
    fn test_min_prune_tokens_skips_small_outputs() {
        let short_lines = "x\n".repeat(25);
        let (call, res) = make_tool_turn("c1", "bash", "echo", &short_lines);
        let history = vec![
            Message::user("Run echo"),
            call,
            res,
            Message::assistant("Done echo"),
            Message::user("Next step"),
        ];

        let policy_skip_small = PrunePolicy {
            volatile_turns: 1,
            line_threshold: DEFAULT_PRUNE_LINE_THRESHOLD,
            protect_recent_tokens: 0,
            prune_minimum_tokens: 0,
            min_prune_tokens: 100,
            model: None,
        };
        let pruned = prune_historical_tool_outputs_with_policy(&history, &policy_skip_small);
        assert_eq!(pruned, history);
    }

    #[test]
    fn test_prune_policy_for_context_window() {
        let p_128k = PrunePolicy::for_context_window(128_000);
        assert_eq!(p_128k.prune_minimum_tokens, 6_400);
        assert_eq!(p_128k.protect_recent_tokens, 19_200);

        let p_200k = PrunePolicy::for_context_window(200_000);
        assert_eq!(p_200k.prune_minimum_tokens, 10_000);
        assert_eq!(p_200k.protect_recent_tokens, 30_000);

        let p_1m = PrunePolicy::for_context_window(1_000_000);
        assert_eq!(p_1m.prune_minimum_tokens, 20_000);
        assert_eq!(p_1m.protect_recent_tokens, 30_000);

        let p_tiny = PrunePolicy::for_context_window(32_000);
        assert_eq!(p_tiny.prune_minimum_tokens, 3_000);
        assert_eq!(p_tiny.protect_recent_tokens, 10_000);
    }

    #[test]
    fn test_adaptive_pruning_floor_triggers_on_smaller_window() {
        let body = "verbose compiler warning line output\n".repeat(1_600);
        let (call, res) = make_tool_turn("c1", "bash", "cargo build", &body);
        let history = vec![
            Message::user("Build"),
            call,
            res,
            Message::assistant("Done build"),
            Message::user("Next command"),
        ];

        let mut static_policy = PrunePolicy::cache_preserving();
        static_policy.protect_recent_tokens = 0;
        let unpruned = prune_historical_tool_outputs_with_policy(&history, &static_policy);
        assert_eq!(unpruned, history);

        let mut adaptive_policy = PrunePolicy::for_context_window(128_000);
        adaptive_policy.protect_recent_tokens = 0;
        let pruned = prune_historical_tool_outputs_with_policy(&history, &adaptive_policy);
        assert_ne!(pruned, history);
    }

    #[test]
    fn test_prune_policy_model_propagation() {
        let policy = PrunePolicy::cache_preserving().with_model("claude-3-7-sonnet");
        assert_eq!(policy.model.as_deref(), Some("claude-3-7-sonnet"));
    }

    #[test]
    fn test_skill_tool_output_never_pruned() {
        let skill_output = "Skill instructions line\n".repeat(50);
        let (call, res) = make_tool_turn("c1", "skill", "coding-standard", &skill_output);
        let history = vec![
            Message::user("Load skill"),
            call,
            res,
            Message::assistant("Skill loaded"),
            Message::user("Next step"),
        ];

        let pruned = prune_historical_tool_outputs(&history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);
        assert_eq!(pruned, history);
    }

    #[test]
    fn test_superseded_file_reads_detected_and_elided() {
        let (read_call1, read_res1) = make_read_turn("c1", "src/lib.rs", &"pub fn foo() {}\n".repeat(20));
        let edit_call = ToolCall::new(
            ToolCallId::new_or_mint("c2"),
            ToolFunction::new(
                "edit".to_string(),
                serde_json::json!({ "path": "src/lib.rs", "content": "edited" }),
            ),
        );
        let edit_res = rig::message::ToolResult {
            call: ToolCallId::new_or_mint("c2"),
            provider: None,
            name: "edit".to_string(),
            content: vec![ToolResultContent::Text(Text::new("Successfully replaced"))],
        };
        let edit_call_msg = Message::Assistant {
            id: None,
            content: vec![AssistantContent::ToolCall(edit_call)],
        };
        let edit_res_msg = Message::User {
            content: vec![UserContent::ToolResult(edit_res)],
        };

        let history = vec![
            Message::user("Read file"),
            read_call1,
            read_res1,
            Message::assistant("I read lib.rs"),
            Message::user("Now edit it"),
            edit_call_msg,
            edit_res_msg,
            Message::assistant("File edited"),
            Message::user("Next step"),
        ];

        let pruned = prune_historical_tool_outputs(&history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);
        let Message::User { content } = &pruned[2] else {
            panic!()
        };
        let UserContent::ToolResult(r) = &content[0] else {
            panic!()
        };
        let ToolResultContent::Text(t) = &r.content[0] else {
            panic!()
        };
        assert_eq!(t.text, "[File read superseded by a later operation on src/lib.rs]");
    }
}
