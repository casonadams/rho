use std::borrow::Borrow;

use rig::message::{Message, UserContent};

use super::estimate_message_tokens;
use crate::session::compaction::CompactionCut;
use crate::session::tree::TreeNodeData;

pub fn is_tool_result_message(message: &Message) -> bool {
    if let Message::User { content } = message {
        content.iter().any(|c| matches!(c, UserContent::ToolResult(_)))
    } else {
        false
    }
}

pub fn is_user_turn_start(message: &Message) -> bool {
    match message {
        Message::User { content } => !content.iter().any(|c| matches!(c, UserContent::ToolResult(_))),
        _ => false,
    }
}

fn scan_backwards_for_token_budget<M: Borrow<Message>>(messages: &[M], keep_tokens: usize, model: &str) -> usize {
    let mut accumulated: usize = 0;
    let mut cut_idx = messages.len();
    for i in (0..messages.len()).rev() {
        accumulated = accumulated.saturating_add(estimate_message_tokens(messages[i].borrow(), model));
        cut_idx = i;
        if accumulated >= keep_tokens {
            break;
        }
    }
    cut_idx
}

fn adjust_for_tool_results<M: Borrow<Message>>(messages: &[M], mut cut_idx: usize) -> usize {
    while cut_idx > 0 && is_tool_result_message(messages[cut_idx].borrow()) {
        cut_idx -= 1;
    }
    cut_idx
}

fn determine_split_turn<M: Borrow<Message>>(messages: &[M], cut_idx: usize) -> bool {
    cut_idx > 0 && cut_idx < messages.len() && !is_user_turn_start(messages[cut_idx].borrow())
}

pub fn find_token_cut_point<M: Borrow<Message>>(
    messages: &[M],
    keep_recent_tokens: usize,
    model: &str,
) -> CompactionCut {
    if messages.is_empty() {
        return CompactionCut {
            cut_index: 0,
            is_split_turn: false,
            first_kept_node_id: None,
            first_kept_message_index: None,
        };
    }

    let cut_idx = scan_backwards_for_token_budget(messages, keep_recent_tokens, model);
    let cut_idx = adjust_for_tool_results(messages, cut_idx);
    let is_split_turn = determine_split_turn(messages, cut_idx);

    CompactionCut {
        cut_index: cut_idx,
        is_split_turn,
        first_kept_node_id: None,
        first_kept_message_index: None,
    }
}

pub fn message_position_at(nodes: &[&TreeNodeData], cut_index: usize) -> Option<(String, usize)> {
    let mut accumulated = 0;
    for node in nodes {
        if cut_index < accumulated + node.messages.len() {
            return Some((node.id.clone(), cut_index - accumulated));
        }
        accumulated += node.messages.len();
    }
    None
}

pub fn find_node_token_cut_point(nodes: &[&TreeNodeData], keep_recent_tokens: usize, model: &str) -> CompactionCut {
    if nodes.is_empty() {
        return CompactionCut {
            cut_index: 0,
            is_split_turn: false,
            first_kept_node_id: None,
            first_kept_message_index: None,
        };
    }

    let messages: Vec<&Message> = nodes.iter().flat_map(|n| &n.messages).collect();
    let mut cut = find_token_cut_point(&messages, keep_recent_tokens, model);
    if cut.cut_index < messages.len()
        && let Some((node_id, msg_idx)) = message_position_at(nodes, cut.cut_index)
    {
        cut.first_kept_node_id = Some(node_id);
        cut.first_kept_message_index = Some(msg_idx);
    }
    cut
}
