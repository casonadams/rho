use super::artifact_spill::DemotionHook;
use super::fit::fit_summary;
use crate::engine::compactor::llm::ModelHandle;
use rho_harness_core::error::Result;
use rho_harness_core::model::ChatMessage;
use rho_harness_core::session::DEFAULT_COMPACTION_MAX_BYTES;
use rho_harness_core::session::compaction::{
    CompactionCut, CompactionDetails, CompactionMetadata, compaction_summary_message, compose_compaction_summary,
    extract_file_ops, render_file_lists_xml,
};
use rho_harness_core::session::tree::{TreeNodeData, TreeNodeKind};
use rho_harness_core::todo::carried_todos;
use rho_harness_core::tokens::{find_token_cut_point, is_tool_result_message};
use std::sync::Arc;

use super::llm::LlmCompactor;
use crate::engine::AgentEngine;
use crate::engine::metrics::{RunTracker, StructuralUsage};
use crate::engine::tracking::{ContextTracker, UsageTracker};
use rho_harness_core::session::SessionManager;

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
    messages: &'a [ChatMessage],
    cut_index: usize,
    is_split_turn: bool,
    prior_summary: Option<&'a str>,
    prior_details: Option<&'a CompactionDetails>,
    instructions: Option<&'a str>,
    tokens_before: usize,
    first_kept_node_id: Option<String>,
    first_kept_message_index: Option<usize>,
}

fn noop_stats(tokens: usize) -> CompactionStats {
    CompactionStats {
        tokens_before: tokens,
        tokens_after: tokens,
        saved_tokens: 0,
        summary: String::new(),
    }
}

/// Folds the older part of an in-memory conversation (one with no session tree, such as a
/// subagent's) into a summary, keeping its most recent `keep_recent_tokens` as they are. The result
/// starts with the summary, like a session's history after compaction. `None` when nothing can be
/// folded or no summary could be written.
pub async fn compact_messages(
    model: &ModelHandle,
    messages: &[ChatMessage],
    keep_recent_tokens: usize,
) -> Option<Vec<ChatMessage>> {
    let cut = find_token_cut_point(messages, keep_recent_tokens, model.model_name());
    if cut.cut_index == 0 {
        return None;
    }
    let (folded, kept) = messages.split_at(cut.cut_index);
    let summary = LlmCompactor::new(Some(model.clone()))
        .summarize(
            folded,
            super::llm::SummarizeOptions {
                prior_summary: None,
                custom_instructions: None,
                is_split_turn: cut.is_split_turn,
                structured: false,
            },
        )
        .await;
    if summary.trim().is_empty() {
        return None;
    }
    let files = extract_file_ops(folded, None);
    let summary = fit_summary(
        &compose_compaction_summary(&summary, &render_file_lists_xml(&files)),
        &carried_todos(None, folded, kept),
        DEFAULT_COMPACTION_MAX_BYTES,
    );
    let mut compacted = vec![compaction_summary_message(&summary)];
    compacted.extend_from_slice(kept);
    Some(compacted)
}

fn resolve_prior_compaction<'a>(
    ancestor_nodes: &'a [&'a TreeNodeData],
) -> (
    Option<String>,
    Option<CompactionDetails>,
    Option<usize>,
    &'a [&'a TreeNodeData],
) {
    let last_idx = ancestor_nodes.iter().rposition(|n| n.kind == TreeNodeKind::Compaction);
    let Some(idx) = last_idx else {
        return (None, None, None, ancestor_nodes);
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
    let kept_msg_idx = meta.as_ref().and_then(|m| m.first_kept_message_index);
    (summary, details, kept_msg_idx, &ancestor_nodes[start_idx..])
}

fn calculate_effective_cut(
    all_msgs: &[ChatMessage],
    positions: &[(String, usize)],
    keep_tokens: usize,
    model: &str,
) -> CompactionCut {
    let mut cut = find_token_cut_point(all_msgs, keep_tokens, model);
    if cut.cut_index == 0 && all_msgs.len() > 1 {
        let mut adjusted_idx = all_msgs.len().saturating_sub(1);
        while adjusted_idx > 0 && is_tool_result_message(&all_msgs[adjusted_idx]) {
            adjusted_idx -= 1;
        }
        cut.cut_index = adjusted_idx;
    }
    if cut.cut_index < positions.len() {
        let (node_id, msg_idx) = &positions[cut.cut_index];
        cut.first_kept_node_id = Some(node_id.clone());
        cut.first_kept_message_index = Some(*msg_idx);
    }
    cut
}

fn compute_post_compaction_tokens(summary: &str, kept: &[ChatMessage], model: &str, context: &ContextTracker) -> usize {
    let summary_msg = compaction_summary_message(summary);
    let mut kept_with_summary = vec![summary_msg];
    kept_with_summary.extend_from_slice(kept);
    context
        .calculate_context_tokens(&kept_with_summary, None, model)
        .total_tokens
}

fn filter_conversation_messages(
    nodes: &[&TreeNodeData],
    first_node_msg_offset: usize,
) -> (Vec<ChatMessage>, Vec<(String, usize)>) {
    let mut msgs = Vec::new();
    let mut positions = Vec::new();
    let mut is_first = true;
    for node in nodes {
        if node.kind != TreeNodeKind::Compaction {
            let offset = if is_first {
                first_node_msg_offset.min(node.messages.len())
            } else {
                0
            };
            is_first = false;
            for (idx, msg) in node.messages[offset..].iter().enumerate() {
                msgs.push(msg.clone());
                positions.push((node.id.clone(), offset + idx));
            }
        }
    }
    (msgs, positions)
}

/// Compaction orchestration over the durable session tree. Shared by the
/// engine's compaction entry points.
#[derive(Clone)]
pub struct SessionCompactor {
    session_manager: SessionManager,
    usage: UsageTracker,
    run_tracker: RunTracker,
    context: ContextTracker,
    model: Option<ModelHandle>,
    model_name: String,
    keep_recent_tokens: usize,
    max_bytes: usize,
    demotion_hook: Option<Arc<dyn DemotionHook>>,
}

#[derive(Clone, Debug)]
pub struct SessionCompactorConfig {
    pub model_name: String,
    pub keep_recent_tokens: usize,
    pub max_bytes: usize,
}

impl SessionCompactor {
    pub fn new(
        session_manager: SessionManager,
        usage: UsageTracker,
        run_tracker: RunTracker,
        context: ContextTracker,
        model: Option<ModelHandle>,
        config: SessionCompactorConfig,
    ) -> Self {
        Self {
            session_manager,
            usage,
            run_tracker,
            context,
            model,
            model_name: config.model_name,
            keep_recent_tokens: config.keep_recent_tokens,
            max_bytes: config.max_bytes,
            demotion_hook: None,
        }
    }

    pub fn with_demotion_hook(mut self, hook: Arc<dyn DemotionHook>) -> Self {
        self.demotion_hook = Some(hook);
        self
    }

    pub async fn compact(&self, instructions: Option<&str>) -> Result<CompactionStats> {
        let tree = self.session_manager.load_tree().await?;
        let Some(active_leaf_id) = &tree.active_leaf_id else {
            return Ok(CompactionStats::empty());
        };
        let ancestor_nodes = tree.ancestor_nodes(active_leaf_id);
        if ancestor_nodes.is_empty() {
            return Ok(CompactionStats::empty());
        }
        self.execute_compaction(&tree, &ancestor_nodes, instructions).await
    }

    async fn execute_compaction(
        &self,
        tree: &rho_harness_core::session::SessionTree,
        ancestor_nodes: &[&TreeNodeData],
        instructions: Option<&str>,
    ) -> Result<CompactionStats> {
        let (prior_sum, prior_det, prior_msg_idx, active_nodes) = resolve_prior_compaction(ancestor_nodes);
        let (all_msgs, positions) = filter_conversation_messages(active_nodes, prior_msg_idx.unwrap_or(0));
        let tokens_before = self
            .context
            .calculate_context_tokens(&tree.active_messages(), None, &self.model_name)
            .total_tokens;
        let cut = calculate_effective_cut(&all_msgs, &positions, self.keep_recent_tokens, &self.model_name);
        if cut.cut_index == 0 || all_msgs.is_empty() {
            return Ok(noop_stats(tokens_before));
        }
        self.finalize_compaction(FinalizeCompactionPlan {
            messages: &all_msgs,
            cut_index: cut.cut_index,
            is_split_turn: cut.is_split_turn,
            prior_summary: prior_sum.as_deref(),
            prior_details: prior_det.as_ref(),
            instructions,
            tokens_before,
            first_kept_node_id: cut.first_kept_node_id,
            first_kept_message_index: cut.first_kept_message_index,
        })
        .await
    }

    async fn generate_compaction_summary(
        &self,
        msgs: &[ChatMessage],
        options: super::llm::SummarizeOptions<'_>,
    ) -> String {
        let compactor = LlmCompactor::new(self.model.clone());
        let (summary, usage) = compactor.summarize_with_usage(msgs, options).await;
        if let Some(u) = usage
            && u.has_values()
        {
            self.usage.record_compaction_usage(u);
        }
        summary
    }

    async fn persist_compaction(&self, metadata: CompactionMetadata) -> Result<()> {
        let tokens_after = metadata.tokens_after;
        self.session_manager
            .append_compaction(&metadata.summary.clone(), metadata)
            .await?;
        self.usage.update_active_context(StructuralUsage {
            input_tokens: tokens_after as u64,
            ..Default::default()
        });
        self.run_tracker.forget_last_request();
        self.context.reset_notices();
        Ok(())
    }

    async fn finalize_compaction(&self, plan: FinalizeCompactionPlan<'_>) -> Result<CompactionStats> {
        let (to_sum, kept) = (&plan.messages[..plan.cut_index], &plan.messages[plan.cut_index..]);
        dispatch_demote(self.demotion_hook.as_ref(), &self.session_manager.session_id, to_sum).await;
        let md_summary = self
            .generate_compaction_summary(
                to_sum,
                super::llm::SummarizeOptions {
                    prior_summary: plan.prior_summary,
                    custom_instructions: plan.instructions,
                    is_split_turn: plan.is_split_turn,
                    structured: false,
                },
            )
            .await;
        let file_details = extract_file_ops(to_sum, plan.prior_details);
        let summary = compose_compaction_summary(&md_summary, &render_file_lists_xml(&file_details));
        let todos = carried_todos(plan.prior_summary, to_sum, kept);
        let final_summary = fit_summary(
            &self.session_manager.redact_credentials(&summary),
            &self.session_manager.redact_credentials(&todos),
            self.max_bytes,
        );

        let tokens_after = compute_post_compaction_tokens(&final_summary, kept, &self.model_name, &self.context);
        let saved_tokens = plan.tokens_before.saturating_sub(tokens_after);
        self.persist_compaction(CompactionMetadata {
            summary: final_summary.clone(),
            first_kept_node_id: plan.first_kept_node_id,
            first_kept_message_index: plan.first_kept_message_index,
            tokens_before: plan.tokens_before,
            tokens_after,
            read_files: file_details.read_files,
            modified_files: file_details.modified_files,
            custom_instructions: plan.instructions.map(str::to_string),
        })
        .await?;

        Ok(CompactionStats {
            tokens_before: plan.tokens_before,
            tokens_after,
            saved_tokens,
            summary: final_summary,
        })
    }
}

impl AgentEngine {
    pub(crate) fn session_compactor(&self) -> SessionCompactor {
        let mut compactor = SessionCompactor::new(
            self.session_manager.clone(),
            self.usage.clone(),
            self.run_tracker.clone(),
            self.context.clone(),
            self.compactor_model.clone(),
            SessionCompactorConfig {
                model_name: self.config.model.clone(),
                keep_recent_tokens: self.config.keep_recent_tokens,
                max_bytes: self.config.compaction_max_bytes,
            },
        );
        if let Some(hook) = &self.demotion_hook {
            compactor = compactor.with_demotion_hook(Arc::clone(hook));
        }
        compactor
    }

    pub async fn compact_session(&self, instructions: Option<&str>) -> Result<CompactionStats> {
        self.promote_unfinished_turn().await?;
        self.session_compactor().compact(instructions).await
    }

    /// Switches the session to another branch; the new context starts its budget notices over.
    pub async fn switch_branch(&self, leaf_id: Option<String>) -> Result<Vec<ChatMessage>> {
        let messages = self.session_manager.switch_branch(leaf_id).await?;
        self.context.reset_notices();
        Ok(messages)
    }

    /// Rewinds the session to `target_turn`; the shorter context starts its budget notices over.
    pub async fn rewind_to_turn(&self, target_turn: usize) -> Result<usize> {
        let retained = self.session_manager.rewind_to_turn(target_turn).await?;
        self.context.reset_notices();
        Ok(retained)
    }

    /// Compaction summarizes canonical history only, so an unfinished turn's saved steps join it first.
    async fn promote_unfinished_turn(&self) -> Result<()> {
        let Some(pending) = self.session_manager.load_checkpoint().await? else {
            return Ok(());
        };
        let closing = match pending.last() {
            Some(ChatMessage::Assistant { .. }) => Vec::new(),
            _ => vec![ChatMessage::assistant(UNFINISHED_TURN_NOTE)],
        };
        self.session_manager.promote_checkpoint(closing).await
    }
}

const UNFINISHED_TURN_NOTE: &str = "[This turn stopped before finishing; the work above was saved.]";

pub(crate) async fn dispatch_demote(hook: Option<&Arc<dyn DemotionHook>>, session_id: &str, messages: &[ChatMessage]) {
    if let Some(hook) = hook {
        if messages.is_empty() {
            return;
        }
        let hook = Arc::clone(hook);
        let fut = std::panic::AssertUnwindSafe(async move { hook.on_demote(session_id, messages).await });
        if futures::FutureExt::catch_unwind(fut).await.is_err() {
            tracing::warn!("demotion hook panicked; compaction continues");
        }
    }
}
