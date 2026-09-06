use rho_harness_core::error::Result;
use rho_harness_core::session::compaction::{
    CompactionCut, CompactionDetails, CompactionMetadata, compaction_summary_message, compose_compaction_summary,
    extract_file_ops, render_file_lists_xml,
};
use rho_harness_core::session::tree::{TreeNodeData, TreeNodeKind};
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

struct FinalizeCompactionPlan<'a> {
    messages: &'a [Message],
    cut_index: usize,
    is_split_turn: bool,
    prior_summary: Option<&'a str>,
    prior_details: Option<&'a CompactionDetails>,
    instructions: Option<&'a str>,
}

fn noop_stats(tokens: usize) -> CompactionStats {
    CompactionStats {
        tokens_before: tokens,
        tokens_after: tokens,
        saved_tokens: 0,
        summary: String::new(),
    }
}

fn resolve_prior_compaction<'a>(
    ancestor_nodes: &'a [&'a TreeNodeData],
) -> (Option<String>, Option<CompactionDetails>, &'a [&'a TreeNodeData]) {
    let last_idx = ancestor_nodes.iter().rposition(|n| n.kind == TreeNodeKind::Compaction);
    let Some(idx) = last_idx else {
        return (None, None, ancestor_nodes);
    };
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
    let start_idx = meta
        .as_ref()
        .and_then(|m| m.first_kept_node_id.as_deref())
        .and_then(|id| ancestor_nodes.iter().position(|n| n.id == id))
        .unwrap_or(idx + 1);
    (summary, details, &ancestor_nodes[start_idx..])
}

fn calculate_effective_cut(
    active_nodes: &[&TreeNodeData],
    all_msgs: &[Message],
    (keep_tokens, model): (usize, &str),
) -> (CompactionCut, Option<String>) {
    let raw = find_node_token_cut_point(active_nodes, keep_tokens, model);
    if raw.cut_index != 0 || active_nodes.len() <= 1 {
        let kept_id = raw.first_kept_node_id.clone();
        return (raw, kept_id);
    }
    let last_node = active_nodes.last().unwrap();
    let last_start = all_msgs.len().saturating_sub(last_node.messages.len());
    let mut adjusted_idx = last_start.max(1);
    while adjusted_idx > 0 && is_tool_result_message(&all_msgs[adjusted_idx]) {
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
}

fn compute_post_compaction_tokens(summary: &str, kept: &[Message], model: &str) -> usize {
    let summary_msg = compaction_summary_message(summary);
    let mut kept_with_summary = vec![summary_msg];
    kept_with_summary.extend_from_slice(kept);
    calculate_context_tokens(&kept_with_summary, None, model).total_tokens
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
        self.execute_compaction((&tree, &ancestor_nodes), instructions).await
    }

    async fn execute_compaction(
        &self,
        (tree, ancestor_nodes): (&rho_harness_core::session::SessionTree, &[&TreeNodeData]),
        instructions: Option<&str>,
    ) -> Result<CompactionStats> {
        let (prior_sum, prior_det, active_nodes) = resolve_prior_compaction(ancestor_nodes);
        let tokens_before = calculate_context_tokens(&tree.active_messages(), None, &self.config.model).total_tokens;
        let all_msgs: Vec<Message> = active_nodes.iter().flat_map(|n| n.messages.clone()).collect();
        let (cut, kept_id) = calculate_effective_cut(
            active_nodes,
            &all_msgs,
            (self.config.keep_recent_tokens, &self.config.model),
        );
        if cut.cut_index == 0 || all_msgs.is_empty() {
            return Ok(noop_stats(tokens_before));
        }
        self.finalize_compaction(
            FinalizeCompactionPlan {
                messages: &all_msgs,
                cut_index: cut.cut_index,
                is_split_turn: cut.is_split_turn,
                prior_summary: prior_sum.as_deref(),
                prior_details: prior_det.as_ref(),
                instructions,
            },
            (tokens_before, kept_id),
        )
        .await
    }

    async fn generate_compaction_summary(
        &self,
        msgs: &[Message],
        (prior_summary, custom_instructions, is_split_turn): (Option<&str>, Option<&str>, bool),
    ) -> String {
        let compactor = LlmCompactor::new(self.model.clone());
        compactor
            .summarize(
                msgs,
                super::llm::SummarizeOptions {
                    prior_summary,
                    custom_instructions,
                    is_split_turn,
                },
            )
            .await
    }

    async fn persist_compaction(
        &self,
        (summary, file_details, kept_id, instructions): (&str, &CompactionDetails, Option<String>, Option<&str>),
        (tokens_before, tokens_after): (usize, usize),
    ) -> Result<()> {
        let metadata = CompactionMetadata {
            summary: summary.to_string(),
            first_kept_node_id: kept_id,
            tokens_before,
            tokens_after,
            read_files: file_details.read_files.clone(),
            modified_files: file_details.modified_files.clone(),
            custom_instructions: instructions.map(str::to_string),
        };
        self.session_manager.append_compaction(summary, metadata).await?;
        self.usage.record(StructuralUsage {
            input_tokens: tokens_after as u64,
            ..Default::default()
        });
        Ok(())
    }

    async fn finalize_compaction(
        &self,
        plan: FinalizeCompactionPlan<'_>,
        (tokens_before, kept_id): (usize, Option<String>),
    ) -> Result<CompactionStats> {
        let (to_sum, kept) = (&plan.messages[..plan.cut_index], &plan.messages[plan.cut_index..]);
        let md_summary = self
            .generate_compaction_summary(to_sum, (plan.prior_summary, plan.instructions, plan.is_split_turn))
            .await;
        let file_details = extract_file_ops(to_sum, plan.prior_details);
        let summary = compose_compaction_summary(&md_summary, &render_file_lists_xml(&file_details));
        let final_summary = self.session_manager.redact_credentials(&summary);

        let tokens_after = compute_post_compaction_tokens(&final_summary, kept, &self.config.model);
        let saved_tokens = tokens_before.saturating_sub(tokens_after);
        self.persist_compaction(
            (&final_summary, &file_details, kept_id, plan.instructions),
            (tokens_before, tokens_after),
        )
        .await?;

        Ok(CompactionStats {
            tokens_before,
            tokens_after,
            saved_tokens,
            summary: final_summary,
        })
    }
}
