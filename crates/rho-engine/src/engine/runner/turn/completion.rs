use crate::engine::AgentEngine;
use crate::engine::metrics::{RunMetrics, TerminalStatus};
use crate::engine::runner::helpers::redact_text;
use crate::engine::runner::sink::TurnArtifacts;
use crate::engine::runner::turn::types::{RunStatus, TurnOutput};
use rho_harness_core::error::{AppError, Result};
use rho_harness_core::session::SessionEventKind;
use rig::completion::FinishReason;

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
        let status = if matches!(error, AppError::ModelBudgetExhausted { .. }) {
            TerminalStatus::BudgetExhausted
        } else if matches!(error, AppError::Cancelled(_)) {
            TerminalStatus::Cancelled
        } else {
            TerminalStatus::Failed
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

    pub(super) async fn finish_turn(&self, artifacts: TurnArtifacts) -> Result<TurnOutput> {
        let TurnArtifacts {
            response,
            tool_calls_count,
            completed_tools,
            generation_elapsed_ms,
        } = artifacts;

        for tool in &completed_tools {
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
                .await?;
        }
        self.session_manager
            .append_event(
                SessionEventKind::AssistantResponse,
                serde_json::json!({ "content": response.output }),
            )
            .await?;

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
        let status = if response
            .completion_calls
            .last()
            .and_then(|call| call.finish_reason.as_ref())
            == Some(&FinishReason::ContentFilter)
        {
            RunStatus::ContentFiltered
        } else {
            RunStatus::Completed
        };

        let requests = response.requests();
        let terminal_status = match status {
            RunStatus::Completed => TerminalStatus::Completed,
            RunStatus::ContentFiltered => TerminalStatus::ContentFiltered,
        };
        let metrics = self.run_tracker.complete(crate::engine::metrics::CompletionOutcome {
            session_id: &self.session_manager.session_id,
            status: terminal_status,
            response: &response,
        });
        self.record_run_summary(&metrics).await?;
        Ok(TurnOutput {
            final_text: response.output,
            tool_calls_count,
            tool_failures_count: completed_tools.iter().filter(|tool| tool.status != "success").count(),
            requests,
            usage: usage_details,
            status,
            metrics,
        })
    }
}
