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

pub fn extract_turns(messages: &[Message]) -> Vec<ConversationTurn> {
    let mut turns = Vec::new();
    let mut current_prompt = String::new();
    let mut current_assistant = String::new();
    let mut current_tool_calls = 0;
    let mut turn_num = 0;

    for msg in messages {
        match msg {
            Message::User { content } => {
                let has_text = content.iter().any(|c| matches!(c, rig::message::UserContent::Text(_)));
                if has_text && (!current_prompt.is_empty() || !current_assistant.is_empty()) {
                    turn_num += 1;
                    turns.push(ConversationTurn {
                        turn_number: turn_num,
                        user_prompt: std::mem::take(&mut current_prompt),
                        assistant_preview: std::mem::take(&mut current_assistant),
                        tool_calls_count: current_tool_calls,
                    });
                    current_tool_calls = 0;
                }
                for part in content {
                    if let rig::message::UserContent::Text(t) = part {
                        if !current_prompt.is_empty() {
                            current_prompt.push(' ');
                        }
                        current_prompt.push_str(&t.text);
                    }
                }
            }
            Message::Assistant { content, .. } => {
                for part in content {
                    match part {
                        rig::message::AssistantContent::Text(t) => {
                            if !current_assistant.is_empty() {
                                current_assistant.push(' ');
                            }
                            current_assistant.push_str(&t.text);
                        }
                        rig::message::AssistantContent::ToolCall(_) => {
                            current_tool_calls += 1;
                        }
                        _ => {}
                    }
                }
            }
            Message::System { .. } => {}
        }
    }

    if !current_prompt.is_empty() || !current_assistant.is_empty() {
        turn_num += 1;
        turns.push(ConversationTurn {
            turn_number: turn_num,
            user_prompt: current_prompt,
            assistant_preview: current_assistant,
            tool_calls_count: current_tool_calls,
        });
    }

    turns
}

pub fn calculate_rewind_cutoff(messages: &[Message], target_turn: usize) -> usize {
    let mut user_turn_count = 0;
    let mut cutoff_idx = 0;

    for (i, msg) in messages.iter().enumerate() {
        if matches!(msg, Message::User { content } if content.iter().any(|c| matches!(c, rig::message::UserContent::Text(_))))
        {
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
