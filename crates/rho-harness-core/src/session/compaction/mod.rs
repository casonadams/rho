pub mod fallback;
pub mod files;
pub mod prompts;
pub mod serialize;
pub mod types;

#[cfg(test)]
mod tests;

pub use fallback::generate_fallback_summary;
pub use files::{extract_file_ops, normalize_path, render_file_lists_xml};
pub use prompts::{
    SUMMARIZATION_PROMPT, SUMMARIZATION_SYSTEM_PROMPT, TURN_PREFIX_SUMMARIZATION_PROMPT, UPDATE_SUMMARIZATION_PROMPT,
    build_summarization_prompt, build_turn_prefix_prompt, build_update_summarization_prompt,
    compose_compaction_summary, merge_split_turn_summary,
};
pub use serialize::{MAX_TOOL_RESULT_CHARS, serialize_conversation};
pub use types::{CompactionCut, CompactionDetails, CompactionMetadata, compaction_summary_message};

use super::SessionManager;
use super::format::{SessionRecord, append_durable_record};
use super::fs::session_error;
use super::tree::{TreeNodeData, TreeNodeKind};
use crate::error::Result;
use chrono::Utc;

impl SessionManager {
    pub async fn append_compaction(&self, summary: &str, metadata: CompactionMetadata) -> Result<()> {
        self.reject_secrets(&summary)?;
        self.reject_secrets(&metadata)?;
        let mut state = self.state.lock().await;
        if state.checkpoint.is_some() {
            return Err(session_error(
                "pending run checkpoint must be continued before appending compaction",
            ));
        }
        let parent_id = state.tree.active_leaf_id.clone();
        let node_id = uuid::Uuid::new_v4().to_string();
        let summary_message = compaction_summary_message(summary);
        let metadata_value = serde_json::to_value(&metadata).map_err(|err| session_error(err.to_string()))?;
        let node = TreeNodeData {
            id: node_id,
            parent_id,
            timestamp: Utc::now(),
            kind: TreeNodeKind::Compaction,
            messages: vec![summary_message],
            label: Some("Compaction".to_string()),
            metadata: Some(metadata_value),
        };
        let record = SessionRecord::TreeNode {
            sequence: state.next_sequence,
            session_id: self.session_id.clone(),
            node: node.clone(),
        };
        append_durable_record(&self.file_path, &record).await?;
        state.next_sequence += 1;
        state.tree.add_node(node);
        state.messages = state.tree.active_messages();
        state.checkpoint = None;
        Ok(())
    }
}
