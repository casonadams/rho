use std::collections::{HashMap, HashSet};

use rho_harness_core::model::{
    AssistantContent, ChatMessage, ImageContent, ToolCall, ToolResult, ToolResultContent, UserContent,
};
use rho_harness_core::tokens::cut_point::is_user_turn_start;

mod cache;
mod stubs;

pub use cache::{PromptCache, PruneMemory};

use stubs::{
    ToolCallMeta, collect_tool_calls, prune_bash_result, prune_image_tool_result, prune_mcp_result, prune_read_result,
    prune_search_result, prune_web_result,
};

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
    let mut new_content: Option<Vec<UserContent>> = None;
    for (idx, item) in content.iter().enumerate() {
        if let Some(pruned) = prune_tool_result_item(item, ctx) {
            new_content.get_or_insert_with(|| content.to_vec())[idx] = pruned;
        }
    }
    new_content
}

pub fn prune_historical_tool_outputs_with_policy(messages: &[ChatMessage], policy: &PrunePolicy) -> Vec<ChatMessage> {
    prune_historical_tool_outputs_if_changed(messages, policy).unwrap_or_else(|| messages.to_vec())
}

pub fn prune_historical_tool_outputs_if_changed(
    messages: &[ChatMessage],
    policy: &PrunePolicy,
) -> Option<Vec<ChatMessage>> {
    if messages.is_empty() {
        return None;
    }

    let model = policy.tokenizer_model();
    let sizes = rho_harness_core::tokens::estimate_each_message_tokens(messages, model);
    prune_historical_tool_outputs_with_sizes(messages, &sizes, policy)
}

/// `sizes[i]` must equal the token estimate for `messages[i]` under `policy.model`.
pub fn prune_historical_tool_outputs_with_sizes(
    messages: &[ChatMessage],
    sizes: &[usize],
    policy: &PrunePolicy,
) -> Option<Vec<ChatMessage>> {
    if messages.is_empty() {
        return None;
    }
    assert_eq!(sizes.len(), messages.len(), "one size per message is required");

    let model = policy.tokenizer_model();
    let cutoff_idx = find_turn_boundary_cutoff(messages, policy.volatile_turns);
    let recent_cutoff_idx = find_recent_tokens_cutoff(sizes, policy.protect_recent_tokens);
    let effective_cutoff = cutoff_idx.min(recent_cutoff_idx);
    if effective_cutoff == 0 {
        return None;
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
    let replacements: Vec<(usize, ChatMessage)> = messages[..effective_cutoff]
        .iter()
        .enumerate()
        .filter_map(|(idx, msg)| match msg {
            ChatMessage::User { content } => prune_user_message_content(content, &ctx).map(|new_content| {
                let new_msg = ChatMessage::User { content: new_content };
                let new_tokens = rho_harness_core::tokens::estimate_message_tokens(&new_msg, model);
                total_savings = total_savings.saturating_add(sizes[idx].saturating_sub(new_tokens));
                (idx, new_msg)
            }),
            _ => None,
        })
        .collect();

    if replacements.is_empty() || (policy.prune_minimum_tokens > 0 && total_savings < policy.prune_minimum_tokens) {
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
