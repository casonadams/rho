use super::SessionManager;
use crate::error::Result;
use rig::message::Message;
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

    fn push_user_part(&mut self, part: &rig::message::UserContent) {
        if let rig::message::UserContent::Text(t) = part {
            if !self.prompt.is_empty() {
                self.prompt.push(' ');
            }
            self.prompt.push_str(&t.text);
        }
    }

    fn push_assistant_part(&mut self, part: &rig::message::AssistantContent) {
        match part {
            rig::message::AssistantContent::Text(t) => {
                if !self.assistant.is_empty() {
                    self.assistant.push(' ');
                }
                self.assistant.push_str(&t.text);
            }
            rig::message::AssistantContent::ToolCall(_) => self.tool_calls += 1,
            _ => {}
        }
    }

    fn process_message(&mut self, msg: &Message) {
        match msg {
            Message::User { content } => {
                let has_text = content.iter().any(|c| matches!(c, rig::message::UserContent::Text(_)));
                if has_text {
                    self.flush();
                }
                for part in content {
                    self.push_user_part(part);
                }
            }
            Message::Assistant { content, .. } => {
                for part in content {
                    self.push_assistant_part(part);
                }
            }
            Message::System { .. } => {}
        }
    }
}

pub fn extract_turns(messages: &[Message]) -> Vec<ConversationTurn> {
    let mut acc = TurnAccumulator::new();
    for msg in messages {
        acc.process_message(msg);
    }
    acc.flush();
    acc.turns
}

fn is_user_text_turn(msg: &Message) -> bool {
    matches!(msg, Message::User { content } if content.iter().any(|c| matches!(c, rig::message::UserContent::Text(_))))
}

pub fn calculate_rewind_cutoff(messages: &[Message], target_turn: usize) -> usize {
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

    pub async fn rewind_to_turn(&self, target_turn: usize) -> Result<usize> {
        let messages = self.load_messages().await?;
        let cutoff_idx = calculate_rewind_cutoff(&messages, target_turn);

        if cutoff_idx == 0 || cutoff_idx >= messages.len() {
            return Ok(messages.len());
        }

        let retained = messages[..cutoff_idx].to_vec();
        self.clear_messages(&self.session_id).await?;
        if !retained.is_empty() {
            self.append_messages(&self.session_id, retained.clone()).await?;
        }
        Ok(retained.len())
    }
}
