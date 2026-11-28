use super::SessionManager;
use super::append::{Planned, tree_node};
use super::format::{Durability, SessionRecord, StoreState};
use super::fs::session_error;
use super::tree::{SessionTree, TreeNodeData, TreeNodeKind};
use crate::error::Result;
use crate::model::{AssistantContent, ChatMessage, UserContent};
use chrono::Utc;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConversationTurn {
    pub turn_number: usize,
    pub user_prompt: String,
    pub assistant_preview: String,
    pub tool_calls_count: usize,
}

struct TurnAccumulator {
    turns: Vec<ConversationTurn>,
    prompt: String,
    assistant: String,
    tool_calls: usize,
    turn_num: usize,
}

impl TurnAccumulator {
    fn new() -> Self {
        Self {
            turns: Vec::new(),
            prompt: String::new(),
            assistant: String::new(),
            tool_calls: 0,
            turn_num: 0,
        }
    }

    fn flush(&mut self) {
        if !self.prompt.is_empty() || !self.assistant.is_empty() {
            self.turn_num += 1;
            self.turns.push(ConversationTurn {
                turn_number: self.turn_num,
                user_prompt: std::mem::take(&mut self.prompt),
                assistant_preview: std::mem::take(&mut self.assistant),
                tool_calls_count: std::mem::take(&mut self.tool_calls),
            });
        }
    }

    fn push_user_part(&mut self, part: &UserContent) {
        if let UserContent::Text(t) = part {
            if !self.prompt.is_empty() {
                self.prompt.push(' ');
            }
            self.prompt.push_str(&t.text);
        }
    }

    fn push_assistant_part(&mut self, part: &AssistantContent) {
        match part {
            AssistantContent::Text(t) => {
                if !self.assistant.is_empty() {
                    self.assistant.push(' ');
                }
                self.assistant.push_str(&t.text);
            }
            AssistantContent::ToolCall(_) => self.tool_calls += 1,
            _ => {}
        }
    }

    fn process_message(&mut self, msg: &ChatMessage) {
        match msg {
            ChatMessage::User { content } => {
                let has_text = content.iter().any(|c| matches!(c, UserContent::Text(_)));
                if has_text {
                    self.flush();
                }
                for part in content {
                    self.push_user_part(part);
                }
            }
            ChatMessage::Assistant { content, .. } => {
                for part in content {
                    self.push_assistant_part(part);
                }
            }
            ChatMessage::System { .. } => {}
        }
    }
}

pub fn extract_turns(messages: &[ChatMessage]) -> Vec<ConversationTurn> {
    let mut acc = TurnAccumulator::new();
    for msg in messages {
        acc.process_message(msg);
    }
    acc.flush();
    acc.turns
}

fn is_user_text_turn(msg: &ChatMessage) -> bool {
    matches!(msg, ChatMessage::User { content } if content.iter().any(|c| matches!(c, UserContent::Text(_))))
}

pub fn calculate_rewind_cutoff(messages: &[ChatMessage], target_turn: usize) -> usize {
    let mut user_turn_count = 0;
    let mut cutoff_idx = 0;

    for (i, msg) in messages.iter().enumerate() {
        if is_user_text_turn(msg) {
            user_turn_count += 1;
            if user_turn_count > target_turn {
                break;
            }
        }
        cutoff_idx = i + 1;
    }

    cutoff_idx
}

impl SessionManager {
    pub async fn load_turns(&self) -> Result<Vec<ConversationTurn>> {
        let messages = self.load_messages().await?;
        Ok(extract_turns(&messages))
    }

    /// Makes the history end at `target_turn` and returns how many messages it keeps. The
    /// discarded turns stay in the tree as a branch.
    pub async fn rewind_to_turn(&self, target_turn: usize) -> Result<usize> {
        self.append(Durability::Durable, move |state, session_id| {
            let cutoff = calculate_rewind_cutoff(&state.messages, target_turn);
            if cutoff == 0 || cutoff >= state.messages.len() {
                return Ok(Planned {
                    records: Vec::new(),
                    output: state.messages.len(),
                });
            }
            if state.checkpoint.is_some() {
                return Err(session_error(
                    "pending run checkpoint must be continued before rewinding",
                ));
            }
            Ok(Planned {
                records: vec![rewind_record(state, session_id, cutoff)?],
                output: cutoff,
            })
        })
        .await
    }
}

/// The active path's nodes after its last compaction, and how many active messages precede them.
fn uncompacted_tail<'a>(tree: &'a SessionTree, leaf_id: &str) -> (usize, Vec<&'a TreeNodeData>) {
    let mut path = tree.ancestor_nodes(leaf_id);
    match path.iter().rposition(|node| node.kind == TreeNodeKind::Compaction) {
        Some(index) => {
            let compacted = tree.ancestor_messages(&path[index].id).len();
            (compacted, path.split_off(index + 1))
        }
        None => (0, path),
    }
}

/// At a node boundary the active leaf moves back to the last kept node; inside a node, a sibling
/// holding the node's kept messages becomes the leaf.
fn rewind_record(state: &StoreState, session_id: &str, cutoff: usize) -> Result<SessionRecord> {
    let leaf_id = state
        .tree
        .active_leaf_id
        .as_deref()
        .ok_or_else(|| session_error("session history has no active node"))?;
    let (mut start, tail) = uncompacted_tail(&state.tree, leaf_id);
    if start + tail.iter().map(|node| node.messages.len()).sum::<usize>() != state.messages.len() {
        return Err(session_error("session history does not match its conversation tree"));
    }
    if cutoff < start {
        return Err(session_error("cannot rewind to a turn before the last compaction"));
    }
    for node in tail {
        let kept = cutoff - start;
        if kept == 0 {
            return Ok(SessionRecord::ActiveLeafChanged {
                sequence: state.next_sequence,
                session_id: session_id.to_string(),
                timestamp: Utc::now(),
                active_leaf_id: node.parent_id.clone(),
            });
        }
        if kept < node.messages.len() {
            let messages = node.messages[..kept].to_vec();
            state.integrity.check_canonical_batch(&messages)?;
            let prefix = TreeNodeData::turn(node.kind.clone(), node.parent_id.clone(), Utc::now(), messages);
            return Ok(tree_node(state.next_sequence, session_id, prefix));
        }
        start += node.messages.len();
    }
    Err(session_error("rewind point is past the end of the session history"))
}
