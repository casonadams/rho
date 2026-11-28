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
use super::fs::session_error;
use super::tree::{TreeNodeData, TreeNodeKind};
use crate::error::Result;
use chrono::Utc;

fn create_compaction_node(
    parent_id: Option<String>,
    summary: &str,
    metadata: &CompactionMetadata,
) -> Result<TreeNodeData> {
    let summary_message = compaction_summary_message(summary);
    let metadata_value = serde_json::to_value(metadata).map_err(|err| session_error(err.to_string()))?;
    Ok(TreeNodeData {
        id: uuid::Uuid::new_v4().to_string(),
        parent_id,
        timestamp: Utc::now(),
        kind: TreeNodeKind::Compaction,
        messages: vec![summary_message],
        label: Some("Compaction".to_string()),
        metadata: Some(metadata_value),
    })
}

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
        let node = create_compaction_node(state.tree.active_leaf_id.clone(), summary, &metadata)?;
        self.append_tree_node(&mut state, node).await?;
        state.checkpoint = None;
        Ok(())
    }
}
