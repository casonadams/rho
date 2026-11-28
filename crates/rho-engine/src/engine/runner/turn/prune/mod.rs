use std::collections::{HashMap, HashSet};

use rho_harness_core::model::{
    AssistantContent, ChatMessage, ImageContent, ToolCall, ToolResult, ToolResultContent, UserContent,
};
use rho_harness_core::tokens::MessageTokenCache;
use rho_harness_core::tokens::cut_point::is_user_turn_start;

mod cache;
mod stubs;

pub use cache::{PromptCache, PruneMemory};

use stubs::{
    ToolCallMeta, collect_tool_calls, prune_bash_result, prune_image_tool_result, prune_mcp_result, prune_read_result,
    prune_search_result, prune_web_result, stub_head,
};

pub const DEFAULT_PRUNE_LINE_THRESHOLD: usize = 15;
pub const DEFAULT_PROTECT_RECENT_TOKENS: usize = 30_000;
pub const DEFAULT_MIN_TOTAL_SAVINGS: usize = 20_000;
pub const DEFAULT_MIN_RESULT_TOKENS: usize = 50;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrunePolicy {
    pub volatile_turns: usize,
    pub line_threshold: usize,
    pub protect_recent_tokens: usize,
    pub min_total_savings: usize,
    pub min_result_tokens: usize,
    pub model: Option<String>,
}

impl Default for PrunePolicy {
    fn default() -> Self {
        Self::cache_preserving()
    }
}

impl PrunePolicy {
    pub const fn cache_preserving() -> Self {
        Self {
            volatile_turns: 1,
            line_threshold: DEFAULT_PRUNE_LINE_THRESHOLD,
            protect_recent_tokens: DEFAULT_PROTECT_RECENT_TOKENS,
            min_total_savings: DEFAULT_MIN_TOTAL_SAVINGS,
            min_result_tokens: DEFAULT_MIN_RESULT_TOKENS,
            model: None,
        }
    }

    pub fn for_context_window(context_window: usize) -> Self {
        let savings_floor = (context_window / 20).clamp(3_000, DEFAULT_MIN_TOTAL_SAVINGS);
        let protect_recent = (context_window * 3 / 20).clamp(10_000, DEFAULT_PROTECT_RECENT_TOKENS);
        Self {
            volatile_turns: 1,
            line_threshold: DEFAULT_PRUNE_LINE_THRESHOLD,
            protect_recent_tokens: protect_recent,
            min_total_savings: savings_floor,
            min_result_tokens: DEFAULT_MIN_RESULT_TOKENS,
            model: None,
        }
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    fn tokenizer_model(&self) -> &str {
        self.model.as_deref().unwrap_or("gpt-4")
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
    let symbol_unset = args.get("symbol").is_none_or(serde_json::Value::is_null);
    let offset_unset = args
        .get("offset")
        .and_then(serde_json::Value::as_u64)
        .is_none_or(|offset| offset <= 1);
    symbol_unset && offset_unset && args.get("limit").is_none_or(serde_json::Value::is_null)
}

fn process_tool_call_path<'a>(
    call: &'a ToolCall,
    results: &HashMap<&str, String>,
    seen_paths: &mut HashSet<&'a str>,
    superseded: &mut HashSet<String>,
) {
    let Some(path) = extract_file_path_arg(&call.function.arguments) else {
        return;
    };
    let name = call.function.name.to_ascii_lowercase();
    let took_effect = results
        .get(call.id.as_str())
        .is_none_or(|text| call_took_effect(&name, text));
    if is_read_tool(&name) {
        if seen_paths.contains(path) {
            superseded.insert(call.id.to_string());
        }
        if took_effect && is_full_read(&call.function.arguments) {
            seen_paths.insert(path);
        }
    } else if is_write_or_edit_tool(&name) && took_effect {
        seen_paths.insert(path);
    }
}

/// Whether a call's recorded result shows it did what its arguments ask: the message carries no
/// error flag, so a failed edit or an error or clipped read is recognized by its text. A result
/// that cannot be found counts as effective.
fn call_took_effect(name: &str, text: &str) -> bool {
    if is_read_tool(name) {
        !stubs::is_read_error(text)
            && !(text.contains("[Showing lines ") && text.contains("Use offset="))
            && !text.contains("[Output truncated")
    } else {
        text.starts_with("Successfully")
    }
}

fn result_texts(messages: &[ChatMessage]) -> HashMap<&str, String> {
    messages
        .iter()
        .filter_map(|msg| match msg {
            ChatMessage::User { content } => Some(content),
            _ => None,
        })
        .flatten()
        .filter_map(|part| match part {
            UserContent::ToolResult(result) => Some((result.call.as_str(), extract_tool_result_text(&result.content))),
            _ => None,
        })
        .collect()
}

pub fn collect_superseded_tool_call_ids(messages: &[ChatMessage]) -> HashSet<String> {
    let results = result_texts(messages);
    let mut seen_paths: HashSet<&str> = HashSet::new();
    let mut superseded = HashSet::new();
    for msg in messages.iter().rev() {
        let ChatMessage::Assistant { content, .. } = msg else {
            continue;
        };
        for item in content.iter().rev() {
            if let AssistantContent::ToolCall(call) = item {
                process_tool_call_path(call, &results, &mut seen_paths, &mut superseded);
            }
        }
    }
    superseded
}

pub fn find_recent_tokens_cutoff(sizes: &[usize], protect_tokens: usize) -> usize {
    if protect_tokens == 0 || sizes.is_empty() {
        return sizes.len();
    }
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
    min_result_tokens: usize,
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
        Some(format!("{}superseded by a later call]", stub_head("read", target)))
    } else if let Some(img) = image_block {
        Some(prune_image_tool_result(tool_name, text, meta, img))
    } else {
        dispatch_tool_prune_stub(tool_name, text, meta, line_threshold)
    }
}

fn prune_tool_result_item(
    item: &UserContent,
    ctx: &PruneContext<'_>,
    memo: &mut MessageTokenCache,
) -> Option<UserContent> {
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

    if ctx.min_result_tokens > 0 && !is_superseded {
        let tokens = memo.get_or_compute_text(&text, ctx.model);
        if tokens < ctx.min_result_tokens {
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

fn prune_user_message_content(
    content: &[UserContent],
    ctx: &PruneContext<'_>,
    memo: &mut MessageTokenCache,
) -> Option<Vec<UserContent>> {
    let mut new_content: Option<Vec<UserContent>> = None;
    for (idx, item) in content.iter().enumerate() {
        if let Some(pruned) = prune_tool_result_item(item, ctx, memo) {
            new_content.get_or_insert_with(|| content.to_vec())[idx] = pruned;
        }
    }
    new_content
}

pub fn prune_historical_tool_outputs_with_policy(messages: &[ChatMessage], policy: &PrunePolicy) -> Vec<ChatMessage> {
    let sizes = rho_harness_core::tokens::estimate_each_message_tokens(messages, policy.tokenizer_model());
    prune_with_token_cache(messages, &sizes, policy, &mut MessageTokenCache::new()).unwrap_or_else(|| messages.to_vec())
}

/// A pass keys each message in the suffix sums, each pruned message, and each tool result text;
/// four keys per message keeps those from evicting one another before the next pass.
const KEYS_PER_MESSAGE: usize = 4;

/// Counts tokens through `memo`, so a caller that keeps it across turns re-tokenizes only new messages.
pub(super) fn prune_with_token_cache(
    messages: &[ChatMessage],
    sizes: &[usize],
    policy: &PrunePolicy,
    memo: &mut MessageTokenCache,
) -> Option<Vec<ChatMessage>> {
    if messages.is_empty() {
        return None;
    }
    debug_assert_eq!(sizes.len(), messages.len(), "one size per message is required");

    let model = policy.tokenizer_model();
    let cutoff_idx = find_turn_boundary_cutoff(messages, policy.volatile_turns);
    let recent_cutoff_idx = find_recent_tokens_cutoff(sizes, policy.protect_recent_tokens);
    let effective_cutoff = cutoff_idx.min(recent_cutoff_idx);
    if effective_cutoff == 0 {
        return None;
    }

    memo.grow_to_fit(messages.len().saturating_mul(KEYS_PER_MESSAGE));
    let tool_calls = collect_tool_calls(messages);
    let superseded_ids = collect_superseded_tool_call_ids(&messages[..effective_cutoff]);
    let ctx = PruneContext {
        tool_calls: &tool_calls,
        superseded_call_ids: &superseded_ids,
        line_threshold: policy.line_threshold,
        min_result_tokens: policy.min_result_tokens,
        model,
    };

    let mut total_savings = 0usize;
    let replacements: Vec<(usize, ChatMessage)> = messages[..effective_cutoff]
        .iter()
        .enumerate()
        .filter_map(|(idx, msg)| match msg {
            ChatMessage::User { content } => prune_user_message_content(content, &ctx, memo).map(|new_content| {
                let new_msg = ChatMessage::User { content: new_content };
                let new_tokens = memo.get_or_compute(&new_msg, model);
                total_savings = total_savings.saturating_add(sizes[idx].saturating_sub(new_tokens));
                (idx, new_msg)
            }),
            _ => None,
        })
        .collect();

    if replacements.is_empty() || (policy.min_total_savings > 0 && total_savings < policy.min_total_savings) {
        return None;
    }

    let mut pruned_messages = messages.to_vec();
    for (idx, new_msg) in replacements {
        pruned_messages[idx] = new_msg;
    }
    Some(pruned_messages)
}

#[cfg(test)]
mod tests;
