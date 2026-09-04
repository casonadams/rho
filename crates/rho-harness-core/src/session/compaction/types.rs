use rig::message::Message;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompactionMetadata {
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_kept_node_id: Option<String>,
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
}

pub fn compaction_summary_message(summary: &str) -> Message {
    Message::System {
        content: summary.to_string(),
    }
}
