use crate::engine::AgentEngine;
use crate::engine::metrics::{RunMetrics, TerminalStatus};
use crate::engine::runner::helpers::redact_text;
use crate::engine::runner::sink::TurnArtifacts;
use crate::engine::runner::turn::types::{RunStatus, TurnOutput, UsageDetails};
use rho_harness_core::error::{AppError, Result};
use rho_harness_core::session::SessionEventKind;
use rig::completion::FinishReason;

fn assemble_turn_output(
    (response, completed_tools, tool_calls_count): (
        rig::agent::PromptResponse,
        &[crate::engine::runner::sink::CompletedTool],
        usize,
    ),
    (usage, status, metrics): (Option<UsageDetails>, RunStatus, RunMetrics),
) -> TurnOutput {
    TurnOutput {
        tool_failures_count: completed_tools.iter().filter(|tool| tool.status != "success").count(),
        requests: response.requests(),
        final_text: response.output,
        tool_calls_count,
        usage,
        status,
        metrics,
    }
}

impl AgentEngine {
    pub async fn record_cancellation(&self, reason: &str) -> Result<()> {
        self.session_manager
            .append_event(
                SessionEventKind::Cancellation,
                serde_json::json!({ "reason": redact_text(reason), "terminal": true }),
            )
            .await?;
        let metrics = self
            .run_tracker
            .terminate(&self.session_manager.session_id, TerminalStatus::Cancelled);
        self.record_run_summary(&metrics).await
    }

    pub(super) async fn record_failed_metrics(&self, error: &AppError) -> Result<()> {
        let status = match error {
            AppError::ModelBudgetExhausted { .. } => TerminalStatus::BudgetExhausted,
            AppError::Cancelled(_) => TerminalStatus::Cancelled,
            _ => TerminalStatus::Failed,
        };
        let metrics = self.run_tracker.terminate(&self.session_manager.session_id, status);
        self.record_run_summary(&metrics).await
    }

    pub(super) async fn record_run_summary(&self, metrics: &RunMetrics) -> Result<()> {
        self.session_manager
            .append_event(
                SessionEventKind::RunSummary,
                serde_json::to_value(metrics).map_err(|error| AppError::Other(error.into()))?,
            )
            .await
    }

    async fn record_single_tool_event(&self, tool: &crate::engine::runner::sink::CompletedTool) -> Result<()> {
        self.session_manager
            .append_event(
                SessionEventKind::ToolCall,
                serde_json::json!({
                    "id": tool.internal_call_id,
                    "name": tool.name,
                    "arguments": tool.arguments,
                }),
            )
            .await?;
        self.session_manager
            .append_event(
                SessionEventKind::ToolResult,
                serde_json::json!({
                    "id": tool.internal_call_id,
                    "name": tool.name,
                    "output": tool.output,
                    "status": tool.status,
                }),
            )
            .await
    }

    async fn record_completed_tool_events(&self, tools: &[crate::engine::runner::sink::CompletedTool]) -> Result<()> {
        for tool in tools {
            self.record_single_tool_event(tool).await?;
        }
        Ok(())
    }

    async fn record_turn_usage(
        &self,
        response: &rig::agent::PromptResponse,
        generation_elapsed_ms: u64,
    ) -> Result<Option<UsageDetails>> {
        let usage = response.usage;
        let usage_details = usage.has_values().then(|| usage.into());
        let latest_context_usage = response.completion_calls.last().map(|call| call.usage).unwrap_or(usage);
        let turn_usage = crate::engine::tracking::TurnUsage::new(usage.into(), latest_context_usage.into());
        self.usage.record_turn(turn_usage, generation_elapsed_ms);
        self.session_manager
            .append_event(
                SessionEventKind::UsageMetrics,
                serde_json::json!({ "available": usage_details.is_some(), "usage": usage_details }),
            )
            .await?;
        Ok(usage_details)
    }

    async fn finalize_turn_summary(
        &self,
        response: &rig::agent::PromptResponse,
        terminal_status: TerminalStatus,
    ) -> Result<RunMetrics> {
        let metrics = self.run_tracker.complete(crate::engine::metrics::CompletionOutcome {
            session_id: &self.session_manager.session_id,
            status: terminal_status,
            response,
        });
        self.record_run_summary(&metrics).await?;
        Ok(metrics)
    }

    pub(super) async fn finish_turn(&self, artifacts: TurnArtifacts) -> Result<TurnOutput> {
        let TurnArtifacts {
            response,
            tool_calls_count,
            completed_tools,
            generation_elapsed_ms,
        } = artifacts;

        self.record_completed_tool_events(&completed_tools).await?;
        self.session_manager
            .append_event(
                SessionEventKind::AssistantResponse,
                serde_json::json!({ "content": response.output }),
            )
            .await?;

        let usage = self.record_turn_usage(&response, generation_elapsed_ms).await?;
        let (status, terminal_status) = determine_turn_status(&response);
        let metrics = self.finalize_turn_summary(&response, terminal_status).await?;
        Ok(assemble_turn_output(
            (response, &completed_tools, tool_calls_count),
            (usage, status, metrics),
        ))
    }

    pub(super) async fn compacted_turn_output(&self) -> Result<TurnOutput> {
        let metrics = self
            .run_tracker
            .terminate(&self.session_manager.session_id, TerminalStatus::Compacted);
        self.record_run_summary(&metrics).await?;
        Ok(TurnOutput {
            final_text: String::new(),
            tool_calls_count: 0,
            tool_failures_count: 0,
            requests: 0,
            usage: None,
            status: RunStatus::Compacted,
            metrics,
        })
    }
}

fn determine_turn_status(response: &rig::agent::PromptResponse) -> (RunStatus, TerminalStatus) {
    let is_filtered = response
        .completion_calls
        .last()
        .and_then(|call| call.finish_reason.as_ref())
        == Some(&FinishReason::ContentFilter);
    if is_filtered {
        (RunStatus::ContentFiltered, TerminalStatus::ContentFiltered)
    } else {
        (RunStatus::Completed, TerminalStatus::Completed)
    }
}
