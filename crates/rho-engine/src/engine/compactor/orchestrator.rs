use rho_harness_core::error::Result;
use rho_harness_core::session::compaction::{
    CompactionCut, CompactionDetails, CompactionMetadata, compaction_summary_message, compose_compaction_summary,
    extract_file_ops, render_file_lists_xml,
};
use rho_harness_core::session::tree::TreeNodeKind;
use rho_harness_core::tokens::{calculate_context_tokens, find_node_token_cut_point, is_tool_result_message};
use rig::message::Message;

use super::llm::LlmCompactor;
use crate::engine::AgentEngine;
use crate::engine::metrics::StructuralUsage;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactionStats {
    pub tokens_before: usize,
    pub tokens_after: usize,
    pub saved_tokens: usize,
    pub summary: String,
}

impl CompactionStats {
    pub fn empty() -> Self {
        Self {
            tokens_before: 0,
            tokens_after: 0,
            saved_tokens: 0,
            summary: String::new(),
        }
    }
}

impl AgentEngine {
    pub async fn compact_session(&self, instructions: Option<&str>) -> Result<CompactionStats> {
        let tree = self.session_manager.load_tree().await?;
        let Some(active_leaf_id) = &tree.active_leaf_id else {
            return Ok(CompactionStats::empty());
        };

        let ancestor_nodes = tree.ancestor_nodes(active_leaf_id);
        if ancestor_nodes.is_empty() {
            return Ok(CompactionStats::empty());
        }

        let last_compaction_idx = ancestor_nodes.iter().rposition(|n| n.kind == TreeNodeKind::Compaction);
        let (prior_summary, prior_details, active_nodes) = match last_compaction_idx {
            Some(idx) => {
                let comp_node = ancestor_nodes[idx];
                let meta = comp_node.compaction_metadata();
                let summary = meta.as_ref().map(|m| m.summary.clone()).or_else(|| {
                    comp_node
                        .metadata
                        .as_ref()
                        .and_then(|v| v.get("summary").and_then(|s| s.as_str()))
                        .map(str::to_string)
                });
                let details = meta.as_ref().map(CompactionDetails::from);
                let first_kept_id = meta.as_ref().and_then(|m| m.first_kept_node_id.as_deref());
                let start_idx = first_kept_id
                    .and_then(|id| ancestor_nodes.iter().position(|n| n.id == id))
                    .unwrap_or(idx + 1);
                (summary, details, &ancestor_nodes[start_idx..])
            }
            None => (None, None, &ancestor_nodes[..]),
        };

        let active_messages = tree.active_messages();
        let tokens_before = calculate_context_tokens(&active_messages, None, &self.config.model).total_tokens;

        let raw_cut = find_node_token_cut_point(active_nodes, self.config.keep_recent_tokens, &self.config.model);
        let all_active_messages: Vec<Message> = active_nodes.iter().flat_map(|n| n.messages.clone()).collect();

        let (cut, first_kept_node_id) = if raw_cut.cut_index == 0 && active_nodes.len() > 1 {
            let last_node = active_nodes.last().unwrap();
            let last_start = all_active_messages.len().saturating_sub(last_node.messages.len());
            let mut adjusted_idx = last_start.max(1);
            while adjusted_idx > 0 && is_tool_result_message(&all_active_messages[adjusted_idx]) {
                adjusted_idx -= 1;
            }
            (
                CompactionCut {
                    cut_index: adjusted_idx,
                    is_split_turn: false,
                    first_kept_node_id: Some(last_node.id.clone()),
                },
                Some(last_node.id.clone()),
            )
        } else {
            let kept_id = raw_cut.first_kept_node_id.clone();
            (raw_cut, kept_id)
        };

        if cut.cut_index == 0 || all_active_messages.is_empty() {
            return Ok(CompactionStats {
                tokens_before,
                tokens_after: tokens_before,
                saved_tokens: 0,
                summary: String::new(),
            });
        }

        let messages_to_summarize = &all_active_messages[..cut.cut_index];
        let messages_kept = &all_active_messages[cut.cut_index..];

        let compactor = LlmCompactor::new(self.model.clone());
        let markdown_summary = compactor
            .summarize(
                messages_to_summarize,
                super::llm::SummarizeOptions {
                    prior_summary: prior_summary.as_deref(),
                    custom_instructions: instructions,
                    is_split_turn: cut.is_split_turn,
                },
            )
            .await;

        let file_details = extract_file_ops(messages_to_summarize, prior_details.as_ref());
        let file_xml = render_file_lists_xml(&file_details);
        let final_summary = compose_compaction_summary(&markdown_summary, &file_xml);
        let final_summary = self.session_manager.redact_credentials(&final_summary);

        let summary_msg = compaction_summary_message(&final_summary);
        let mut kept_with_summary = vec![summary_msg];
        kept_with_summary.extend_from_slice(messages_kept);
        let tokens_after = calculate_context_tokens(&kept_with_summary, None, &self.config.model).total_tokens;
        let saved_tokens = tokens_before.saturating_sub(tokens_after);

        let metadata = CompactionMetadata {
            summary: final_summary.clone(),
            first_kept_node_id,
            tokens_before,
            tokens_after,
            read_files: file_details.read_files,
            modified_files: file_details.modified_files,
            custom_instructions: instructions.map(str::to_string),
        };

        self.session_manager.append_compaction(&final_summary, metadata).await?;
        self.usage.record(StructuralUsage {
            input_tokens: tokens_after as u64,
            ..Default::default()
        });

        Ok(CompactionStats {
            tokens_before,
            tokens_after,
            saved_tokens,
            summary: final_summary,
        })
    }
}
