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

pub fn find_token_cut_point(messages: &[Message], keep_recent_tokens: usize, model: &str) -> CompactionCut {
    if messages.is_empty() {
        return CompactionCut {
            cut_index: 0,
            is_split_turn: false,
            first_kept_node_id: None,
        };
    }

    let mut accumulated_tokens: usize = 0;
    let mut cut_idx = messages.len();

    for i in (0..messages.len()).rev() {
        let msg_tokens = estimate_message_tokens(&messages[i], model);
        accumulated_tokens = accumulated_tokens.saturating_add(msg_tokens);
        cut_idx = i;
        if accumulated_tokens >= keep_recent_tokens {
            break;
        }
    }

    while cut_idx > 0 && is_tool_result_message(&messages[cut_idx]) {
        cut_idx -= 1;
    }

    let is_split_turn = if cut_idx == 0 || cut_idx >= messages.len() {
        false
    } else {
        !is_user_turn_start(&messages[cut_idx])
    };

    CompactionCut {
        cut_index: cut_idx,
        is_split_turn,
        first_kept_node_id: None,
    }
}

pub fn find_node_token_cut_point(nodes: &[&TreeNodeData], keep_recent_tokens: usize, model: &str) -> CompactionCut {
    if nodes.is_empty() {
        return CompactionCut {
            cut_index: 0,
            is_split_turn: false,
            first_kept_node_id: None,
        };
    }

    let mut messages = Vec::new();
    let mut message_node_ids = Vec::new();

    for node in nodes {
        for msg in &node.messages {
            messages.push(msg.clone());
            message_node_ids.push(node.id.clone());
        }
    }

    let mut cut = find_token_cut_point(&messages, keep_recent_tokens, model);
    if cut.cut_index < message_node_ids.len() {
        cut.first_kept_node_id = Some(message_node_ids[cut.cut_index].clone());
    }
    cut
}
