use std::collections::{HashMap, HashSet};

use rho_harness_core::model::{
    AssistantContent, ChatMessage, ImageContent, ToolCall, ToolResult, ToolResultContent, UserContent,
};
use rho_harness_core::tokens::cut_point::is_user_turn_start;

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

fn is_assistant_final_response(message: &ChatMessage) -> bool {
    match message {
        ChatMessage::Assistant { content, .. } => {
            let has_tool_calls = content.iter().any(|c| matches!(c, AssistantContent::ToolCall(_)));
            let has_text = content
                .iter()
                .any(|c| matches!(c, AssistantContent::Text(t) if !t.text.trim().is_empty()));
            !has_tool_calls && has_text
        }
        _ => false,
    }
}

pub fn is_turn_boundary(message: &ChatMessage) -> bool {
    is_user_turn_start(message) || is_assistant_final_response(message)
}

pub fn find_turn_boundary_cutoff(messages: &[ChatMessage], volatile_turns: usize) -> usize {
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

fn collect_tool_calls(messages: &[ChatMessage]) -> HashMap<String, ToolCallMeta> {
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

fn prune_image_tool_result(tool_name: &str, text: &str, meta: Option<&ToolCallMeta>, image: &ImageContent) -> String {
    let target = meta.map(|m| m.target.as_str()).unwrap_or("");
    let media_type = image.media_type.as_deref().unwrap_or("unknown");
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

fn spill_artifact_path(text: &str) -> Option<&str> {
    let (_, notice) = crate::tools::truncate::split_artifact_notice(text);
    let after = &notice?[notice?.find(" saved to ")? + " saved to ".len()..];
    Some(&after[..after.find(". Use '")?])
}

fn with_artifact_path(stub: String, text: &str) -> String {
    match spill_artifact_path(text) {
        Some(path) => format!("{} Full output: {path}]", stub.trim_end_matches(']')),
        None => stub,
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
    let stub = if target.is_empty() {
        format!("[File read ({line_count} lines, {size_str}). Output pruned for historical turn.]")
    } else {
        format!("[File '{target}' read ({line_count} lines, {size_str}). Output pruned for historical turn.]")
    };
    Some(with_artifact_path(stub, text))
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
    let stub = if target.is_empty() {
        format!(
            "[Tool '{tool_name}' completed with {line_count} lines ({size_str}). Output pruned for historical turn.]"
        )
    } else {
        format!(
            "[Tool '{tool_name}' with pattern '{target}' completed with {line_count} lines ({size_str}). Output pruned for historical turn.]"
        )
    };
    Some(with_artifact_path(stub, text))
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

fn is_full_read(args: &serde_json::Value) -> bool {
    let offset_unset = args
        .get("offset")
        .and_then(serde_json::Value::as_u64)
        .is_none_or(|offset| offset <= 1);
    offset_unset && args.get("limit").is_none_or(serde_json::Value::is_null)
}

fn process_tool_call_path<'a>(call: &'a ToolCall, seen_paths: &mut HashSet<&'a str>, superseded: &mut HashSet<String>) {
    let Some(path) = extract_file_path_arg(&call.function.arguments) else {
        return;
    };
    let name = call.function.name.to_ascii_lowercase();
    if is_read_tool(&name) {
        if seen_paths.contains(path) {
            superseded.insert(call.id.to_string());
        }
        if is_full_read(&call.function.arguments) {
            seen_paths.insert(path);
        }
    } else if is_write_or_edit_tool(&name) {
        seen_paths.insert(path);
    }
}

pub fn collect_superseded_tool_call_ids(messages: &[ChatMessage]) -> HashSet<String> {
    let mut seen_paths: HashSet<&str> = HashSet::new();
    let mut superseded = HashSet::new();
    for msg in messages.iter().rev() {
        let ChatMessage::Assistant { content, .. } = msg else {
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

pub fn find_recent_tokens_cutoff(messages: &[ChatMessage], protect_tokens: usize, model: &str) -> usize {
    if protect_tokens == 0 || messages.is_empty() {
        return messages.len();
    }
    let sizes: Vec<usize> = messages
        .iter()
        .map(|msg| rho_harness_core::tokens::estimate_message_tokens(msg, model))
        .collect();
    let total: usize = sizes.iter().sum();
    let step = (protect_tokens / 2).max(1);
    let boundary = total.saturating_sub(protect_tokens) / step * step;
    let mut accumulated = 0usize;
    sizes
        .iter()
        .take_while(|size| {
            accumulated = accumulated.saturating_add(**size);
            accumulated <= boundary
        })
        .count()
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
    image_block: Option<&ImageContent>,
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

    Some(UserContent::ToolResult(ToolResult {
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

pub fn prune_historical_tool_outputs_with_policy(messages: &[ChatMessage], policy: &PrunePolicy) -> Vec<ChatMessage> {
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
    let superseded_ids = collect_superseded_tool_call_ids(&messages[..effective_cutoff]);
    let ctx = PruneContext {
        tool_calls: &tool_calls,
        superseded_call_ids: &superseded_ids,
        line_threshold: policy.line_threshold,
        min_prune_tokens: policy.min_prune_tokens,
        model,
    };

    let mut total_savings = 0usize;
    let pruned_messages: Vec<ChatMessage> = messages
        .iter()
        .enumerate()
        .map(|(idx, msg)| {
            if idx >= effective_cutoff {
                return msg.clone();
            }
            match msg {
                ChatMessage::User { content } => match prune_user_message_content(content, &ctx) {
                    Some(new_content) => {
                        let original_tokens = rho_harness_core::tokens::estimate_message_tokens(msg, model);
                        let new_msg = ChatMessage::User { content: new_content };
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
    messages: &[ChatMessage],
    volatile_turns: usize,
    line_threshold: usize,
) -> Vec<ChatMessage> {
    prune_historical_tool_outputs_with_policy(messages, &PrunePolicy::unconstrained(volatile_turns, line_threshold))
}

#[cfg(test)]
#[path = "prune_tests.rs"]
mod tests;
