use super::SessionManager;
use super::append::{Planned, tree_node};
use super::checkpoint::{promotion_record, unfinished_turn_closing};
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

/// How many messages a rewind kept, and whether it changed the history at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RewindOutcome {
    pub retained: usize,
    pub rewound: bool,
}

impl SessionManager {
    pub async fn load_turns(&self) -> Result<Vec<ConversationTurn>> {
        let messages = self.load_messages().await?;
        Ok(extract_turns(&messages))
    }

    /// Makes the history end at `target_turn`; the discarded turns stay in the tree as a branch. A
    /// pending checkpoint, closed per `unfinished_turn_closing` with `note`, joins the history in the
    /// same append so it is kept with them. Nothing is written when the rewind keeps every turn or is refused.
    pub async fn rewind_to_turn(&self, target_turn: usize, note: ChatMessage) -> Result<RewindOutcome> {
        self.reject_secrets(&note)?;
        self.append(Durability::Durable, move |state, session_id| {
            let Some(checkpoint) = &state.checkpoint else {
                return plan_rewind(state, session_id, target_turn);
            };
            let closing = unfinished_turn_closing(checkpoint, note);
            let promoted: Vec<ChatMessage> = checkpoint.iter().chain(&closing).cloned().collect();
            let history: Vec<ChatMessage> = state.messages.iter().chain(&promoted).cloned().collect();
            let Some(cutoff) = rewind_cutoff(&history, target_turn) else {
                return Ok(keep_everything(state));
            };
            let promotion = promotion_record(state, session_id, checkpoint, closing)?;
            let parent = state.tree.active_leaf_id.clone();
            let unfinished = TreeNodeData::turn(TreeNodeKind::AssistantTurn, parent, Utc::now(), promoted);
            let rewind = rewind_record(state, session_id, cutoff, Some(&unfinished))?;
            Ok(rewound(vec![promotion, rewind], cutoff))
        })
        .await
    }
}

/// Where a rewind to `target_turn` cuts `messages`, or `None` when it would keep all or none of them.
fn rewind_cutoff(messages: &[ChatMessage], target_turn: usize) -> Option<usize> {
    let cutoff = calculate_rewind_cutoff(messages, target_turn);
    (cutoff > 0 && cutoff < messages.len()).then_some(cutoff)
}

fn keep_everything(state: &StoreState) -> Planned<RewindOutcome> {
    Planned {
        records: Vec::new(),
        output: RewindOutcome {
            retained: state.messages.len(),
            rewound: false,
        },
    }
}

fn rewound(records: Vec<SessionRecord>, cutoff: usize) -> Planned<RewindOutcome> {
    Planned {
        records,
        output: RewindOutcome {
            retained: cutoff,
            rewound: true,
        },
    }
}

fn plan_rewind(state: &StoreState, session_id: &str, target_turn: usize) -> Result<Planned<RewindOutcome>> {
    let Some(cutoff) = rewind_cutoff(&state.messages, target_turn) else {
        return Ok(keep_everything(state));
    };
    Ok(rewound(vec![rewind_record(state, session_id, cutoff, None)?], cutoff))
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
/// holding the node's kept messages becomes the leaf. `unfinished` is a turn written just before
/// this record under the active leaf.
fn rewind_record(
    state: &StoreState,
    session_id: &str,
    cutoff: usize,
    unfinished: Option<&TreeNodeData>,
) -> Result<SessionRecord> {
    let (mut start, mut tail) = match state.tree.active_leaf_id.as_deref() {
        Some(leaf_id) => uncompacted_tail(&state.tree, leaf_id),
        None => (0, Vec::new()),
    };
    if start + tail.iter().map(|node| node.messages.len()).sum::<usize>() != state.messages.len() {
        return Err(session_error("session history does not match its conversation tree"));
    }
    tail.extend(unfinished);
    let sequence = state.next_sequence + u64::from(unfinished.is_some());
    if cutoff < start {
        return Err(session_error("cannot rewind to a turn before the last compaction"));
    }
    for node in tail {
        let kept = cutoff - start;
        if kept == 0 {
            return Ok(SessionRecord::ActiveLeafChanged {
                sequence,
                session_id: session_id.to_string(),
                timestamp: Utc::now(),
                active_leaf_id: node.parent_id.clone(),
            });
        }
        if kept < node.messages.len() {
            let messages = node.messages[..kept].to_vec();
            state.integrity.check_canonical_batch(&messages)?;
            let prefix = TreeNodeData::turn(node.kind.clone(), node.parent_id.clone(), Utc::now(), messages);
            return Ok(tree_node(sequence, session_id, prefix));
        }
        start += node.messages.len();
    }
    Err(session_error("rewind point is past the end of the session history"))
}
