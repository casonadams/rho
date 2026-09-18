use rig::message::Message;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompactionMetadata {
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_kept_node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_kept_message_index: Option<usize>,
    #[serde(default)]
    pub tokens_before: usize,
    #[serde(default)]
    pub tokens_after: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub read_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modified_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_instructions: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompactionDetails {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub read_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modified_files: Vec<String>,
}

impl From<&CompactionMetadata> for CompactionDetails {
    fn from(metadata: &CompactionMetadata) -> Self {
        Self {
            read_files: metadata.read_files.clone(),
            modified_files: metadata.modified_files.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompactionCut {
    pub cut_index: usize,
    pub is_split_turn: bool,
    pub first_kept_node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_kept_message_index: Option<usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CompactionSummaryPayload {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(description = "Primary goals and objectives identified in the conversation")]
    pub goals: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(description = "Key technical and architectural decisions made")]
    pub decisions: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(description = "Completed tasks and accomplishments")]
    pub completed: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(description = "Files and paths actively read, edited, or discussed")]
    pub active_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(description = "Open questions, next steps, or unresolved blockers")]
    pub open_questions: Vec<String>,
}

impl CompactionSummaryPayload {
    pub fn render_markdown(&self) -> String {
        super::prompts::render_compaction_payload(self)
    }
}

pub fn compaction_summary_message(summary: &str) -> Message {
    Message::System {
        content: summary.to_string(),
    }
}
