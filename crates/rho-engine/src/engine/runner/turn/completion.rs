use crate::engine::AgentEngine;
use crate::engine::metrics::types::PromptResponse;
use crate::engine::metrics::{RunMetrics, StructuralUsage, TerminalStatus, ToolOutcome};
use crate::engine::runner::helpers::redact_text;
use crate::engine::runner::sink::TurnArtifacts;
use crate::engine::runner::turn::types::TurnOutput;
use crate::provider::adapter::FinishReason;
use rho_harness_core::error::{AppError, Result};
use rho_harness_core::presentation::TurnTelemetry;
use rho_harness_core::session::SessionEventKind;

impl AgentEngine {
    pub async fn record_cancellation(&self, reason: &str) -> Result<()> {
        self.inbox().cancel_jobs();
        self.session_manager
            .append_event(
                SessionEventKind::Cancellation,
                serde_json::json!({ "reason": redact_text(reason), "terminal": true }),
            )
            .await?;
        self.terminate_turn(TerminalStatus::Cancelled, None).await
    }

    pub(super) async fn record_failed_turn(
        &self,
        error: &AppError,
        sink: &crate::engine::runner::sink::TerminalApprovalSink,
    ) -> Result<()> {
        sink.record_tool_events().await?;
        self.session_manager
            .append_event(
                SessionEventKind::RunFailed,
                serde_json::json!({ "error": redact_text(&error.to_string()) }),
            )
            .await?;
        self.record_failed_metrics(error).await
    }

    pub(super) async fn record_failed_metrics(&self, error: &AppError) -> Result<()> {
        let status = match error {
            AppError::ModelBudgetExhausted { .. } => TerminalStatus::BudgetExhausted,
            AppError::Cancelled(_) => TerminalStatus::Cancelled,
            _ => TerminalStatus::Failed,
        };
        let error = (status == TerminalStatus::Failed).then(|| redact_text(&error.to_string()));
        self.terminate_turn(status, error).await
    }

    async fn terminate_turn(&self, status: TerminalStatus, error: Option<String>) -> Result<()> {
        match self.run_tracker.terminate(&self.session_manager.session_id, status) {
            Some(metrics) => self.end_turn(&metrics, metrics.tool_calls, error).await,
            None => Ok(()),
        }
    }

    async fn end_turn(&self, metrics: &RunMetrics, tool_calls_count: usize, error: Option<String>) -> Result<()> {
        let recorded = self.record_run_summary(metrics).await;
        let hook = crate::locking::lock(&self.active_turn_hook).take();
        if let Some(hook) = hook {
            hook.notify_turn_end(metrics.terminal_status, tool_calls_count, error)
                .await;
        }
        recorded
    }

    async fn record_run_summary(&self, metrics: &RunMetrics) -> Result<()> {
        self.session_manager
            .append_event(
                SessionEventKind::RunSummary,
                serde_json::to_value(metrics).map_err(|error| AppError::Other(error.into()))?,
            )
            .await
    }

    async fn record_turn_usage(
        &self,
        response: &PromptResponse,
        generation_elapsed_ms: u64,
    ) -> Result<Option<StructuralUsage>> {
        let usage = response.usage;
        let usage_details = usage.is_reported().then_some(usage);
        let latest_context_usage = response.completion_calls.last().map(|call| call.usage).unwrap_or(usage);
        let turn_usage = crate::engine::tracking::TurnUsage::new(usage, latest_context_usage);
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
        response: &PromptResponse,
        terminal_status: TerminalStatus,
        tool_calls_count: usize,
    ) -> Result<RunMetrics> {
        let metrics = self.run_tracker.complete(crate::engine::metrics::CompletionOutcome {
            session_id: &self.session_manager.session_id,
            status: terminal_status,
            response,
        });
        self.end_turn(&metrics, tool_calls_count, None).await?;
        Ok(metrics)
    }

    fn build_turn_telemetry(&self, artifacts: &TurnArtifacts, usage: StructuralUsage) -> TurnTelemetry {
        let timestamp_unix_ms = u64::try_from(crate::clock::unix_now_ms()).unwrap_or(0);
        TurnTelemetry {
            timestamp_unix_ms,
            turn_elapsed_ms: artifacts.turn_started_at.elapsed().as_millis() as u64,
            request_duration_ms: artifacts.generation_elapsed_ms,
            ttft_ms: artifacts.ttft_ms,
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            cache_read_tokens: usage.cached_input_tokens.unwrap_or(0),
            cache_write_tokens: usage.cache_creation_input_tokens.unwrap_or(0),
            tokens_per_second: TurnTelemetry::compute_tokens_per_second(
                usage.output_tokens,
                artifacts.generation_elapsed_ms,
            ),
            cache_miss_tokens: artifacts.cache_miss_tokens,
        }
    }

    pub(super) async fn finish_turn(
        &self,
        artifacts: TurnArtifacts,
        lifecycle_hook: &crate::hook::LifecycleHook,
    ) -> Result<TurnOutput> {
        self.session_manager
            .append_event(
                SessionEventKind::AssistantResponse,
                serde_json::json!({ "content": artifacts.response.output() }),
            )
            .await?;

        let usage = self
            .record_turn_usage(&artifacts.response, artifacts.generation_elapsed_ms)
            .await?;
        let status = determine_turn_status(&artifacts);
        let metrics = self
            .finalize_turn_summary(&artifacts.response, status, artifacts.tool_calls_count)
            .await?;
        self.spawn_local_context_probe();

        if let Some(reported) = usage {
            let telemetry = self.build_turn_telemetry(&artifacts, reported);
            lifecycle_hook.presenter().print_turn_telemetry(&telemetry);
        }

        let TurnArtifacts {
            response,
            tool_calls_count,
            completed_tools,
            ..
        } = artifacts;
        Ok(TurnOutput {
            tool_failures_count: completed_tools
                .iter()
                .filter(|tool| tool.status != ToolOutcome::Success)
                .count(),
            requests: response.requests,
            final_text: response.output,
            tool_calls_count,
            usage,
            status,
            metrics,
        })
    }
}

fn determine_turn_status(artifacts: &TurnArtifacts) -> TerminalStatus {
    let finished_filtered = artifacts
        .response
        .completion_calls
        .last()
        .and_then(|call| call.finish_reason.as_ref())
        == Some(&FinishReason::ContentFilter);
    if artifacts.content_filtered || finished_filtered {
        TerminalStatus::ContentFiltered
    } else {
        TerminalStatus::Completed
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use rho_harness_core::presentation::activity::ActivityToken;
    use rho_harness_core::presentation::presenter::Presenter;
    use rho_harness_core::presentation::stream::ToolStreamPort;
    use rho_harness_core::presentation::{SessionStatus, ToolLine, TurnTelemetry, WelcomeDisplay};

    use super::*;
    use crate::engine::eval::mock::{MockEngineConfig, mock_engine};
    use crate::engine::metrics::types::{CompletionCall, StructuralUsage};
    use crate::provider::adapter::MockAdapter;

    #[derive(Default)]
    struct TelemetryCapturingPresenter {
        captured: Mutex<Vec<TurnTelemetry>>,
    }

    impl TelemetryCapturingPresenter {
        fn take(&self) -> Vec<TurnTelemetry> {
            std::mem::take(&mut *self.captured.lock().unwrap())
        }
    }

    #[async_trait]
    impl Presenter for TelemetryCapturingPresenter {
        fn write_output(&self, _text: &str) {}
        fn print_welcome(&self, _display: &WelcomeDisplay) {}
        fn print_session_status(&self, _display: &SessionStatus) {}
        fn print_notice(&self, _text: &str) {}
        fn print_user_block(&self, _input: &str) {}
        fn print_token(&self, _token: &str) {}
        fn print_thinking_token(&self, _token: &str) {}
        fn finish_tool_line(&self, _line: ToolLine) {}
        fn flush(&self) {}
        fn has_interactive_ui(&self) -> bool {
            false
        }
        fn start_spinner(&self, _message: &str) -> ActivityToken {
            ActivityToken::default()
        }
        fn start_tool_spinner(&self, _name: &str, _arguments: &serde_json::Value) -> ActivityToken {
            ActivityToken::default()
        }
        fn start_tool_run(&self, _name: &str, _arguments: &serde_json::Value) {}
        fn stream_port(&self) -> ToolStreamPort {
            ToolStreamPort::default()
        }
        fn print_turn_telemetry(&self, telemetry: &TurnTelemetry) {
            self.captured.lock().unwrap().push(telemetry.clone());
        }
    }

    fn usage_with(inp: u64, out: u64, cr: Option<u64>, cw: Option<u64>) -> StructuralUsage {
        StructuralUsage {
            input_tokens: inp,
            output_tokens: out,
            total_tokens: inp + out,
            cached_input_tokens: cr,
            cache_creation_input_tokens: cw,
            tool_use_prompt_tokens: None,
            reasoning_tokens: None,
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn turn_end_hook_receives_the_snake_case_terminal_status() {
        let cases = [
            ("completed", false, None),
            ("content_filtered", true, None),
            ("content_filtered", false, Some(FinishReason::ContentFilter)),
        ];
        for (expected, content_filtered, finish_reason) in cases {
            let dir = rho_test_support::TestDir::new();
            let hook = dir.join("turn_end");
            crate::test_support::write_file(&hook, "#!/bin/sh\ncat > \"$(dirname \"$0\")/payload.json\"\n");
            std::fs::set_permissions(&hook, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
            let engine = mock_engine(
                Arc::new(MockAdapter::default()),
                MockEngineConfig::new(&dir, rho_harness_core::config::Config::default()),
            );
            let lifecycle = Arc::new(
                crate::hook::LifecycleHook::new(
                    dir.to_path_buf(),
                    "s1",
                    Arc::new(crate::engine::eval::presenter::NoopPresenter),
                )
                .with_hook_dirs(vec![dir.to_path_buf()]),
            );
            *engine.active_turn_hook.lock().unwrap() = Some(lifecycle.clone());
            let usage = usage_with(10, 5, None, None);
            let call = CompletionCall::new(0, usage).with_finish_reason(finish_reason);
            let _ = engine
                .finish_turn(
                    TurnArtifacts {
                        response: PromptResponse::new("ok", usage).with_completion_calls(vec![call]),
                        tool_calls_count: 0,
                        completed_tools: Vec::new(),
                        generation_elapsed_ms: 10,
                        turn_started_at: std::time::Instant::now(),
                        ttft_ms: None,
                        cache_miss_tokens: None,
                        content_filtered,
                    },
                    &lifecycle,
                )
                .await
                .unwrap();
            let payload: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(dir.join("payload.json")).unwrap()).unwrap();
            assert_eq!(payload["status"], expected);
        }
    }

    #[cfg(unix)]
    fn engine_with_turn_end_log(
        dir: &rho_test_support::TestDir,
        model: Arc<dyn crate::provider::adapter::ModelAdapter>,
        max_turns: usize,
    ) -> AgentEngine {
        let hook = dir.join("turn_end");
        crate::test_support::write_file(
            &hook,
            "#!/bin/sh\nlog=\"$(dirname \"$0\")/turn_end.jsonl\"\ncat >> \"$log\"\necho >> \"$log\"\n",
        );
        std::fs::set_permissions(&hook, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        let config = rho_harness_core::config::Config {
            auth_file: dir.join("auth.json"),
            hook_dirs: vec![dir.to_path_buf()],
            max_turns,
            ..rho_harness_core::config::Config::default()
        };
        mock_engine(model, MockEngineConfig::new(dir, config))
    }

    #[cfg(unix)]
    fn turn_end_payloads(dir: &rho_test_support::TestDir) -> Vec<serde_json::Value> {
        std::fs::read_to_string(dir.join("turn_end.jsonl"))
            .unwrap_or_default()
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_turn_that_ends_without_success_reports_its_status_to_turn_end_once() {
        use crate::engine::runner::turn::types::{CancellationSignal, TurnRequest};
        use crate::provider::adapter::ModelStreamEvent;
        use rho_harness_core::model::{ToolCall, ToolFunction};
        let missing_tool = ModelStreamEvent::ToolCall(ToolCall::new(
            "call-1",
            ToolFunction::new("missing", serde_json::json!({})),
        ));
        let cases = [
            (
                "failed",
                Err(AppError::Provider("model exploded".into())),
                false,
                5,
                Some("Provider error: model exploded"),
            ),
            ("cancelled", Ok(ModelStreamEvent::Text("hi".into())), true, 5, None),
            ("budget_exhausted", Ok(missing_tool), false, 1, None),
        ];
        for (expected, event, cancelled, max_turns, error) in cases {
            let dir = rho_test_support::TestDir::new();
            let model = MockAdapter::default();
            model.add_turn(vec![event]);
            let engine = engine_with_turn_end_log(&dir, Arc::new(model), max_turns);
            let signal = CancellationSignal::default();
            if cancelled {
                signal.cancel();
            }
            let request = TurnRequest::new("go").with_cancellation(&signal);

            let result = engine
                .run_turn(request, Arc::new(crate::engine::eval::presenter::NoopPresenter))
                .await;

            assert!(result.is_err(), "{expected}: {result:?}");
            let payloads = turn_end_payloads(&dir);
            assert_eq!(payloads.len(), 1, "{expected}: {payloads:?}");
            assert_eq!(payloads[0]["status"], expected);
            assert_eq!(payloads[0]["error"].as_str(), error, "{expected}");
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn cancelling_a_turn_that_already_completed_does_not_report_it_again() {
        let dir = rho_test_support::TestDir::new();
        let engine = engine_with_turn_end_log(&dir, Arc::new(MockAdapter::text("done")), 5);
        engine
            .run_turn(
                crate::engine::runner::turn::types::TurnRequest::new("go"),
                Arc::new(crate::engine::eval::presenter::NoopPresenter),
            )
            .await
            .unwrap();

        engine.record_cancellation("rpc abort").await.unwrap();

        let payloads = turn_end_payloads(&dir);
        assert_eq!(payloads.len(), 1, "{payloads:?}");
        assert_eq!(payloads[0]["status"], "completed");
        assert!(payloads[0].get("error").is_none());
    }

    #[cfg(unix)]
    #[derive(Debug, Default)]
    struct StalledAdapter {
        requested: tokio::sync::Notify,
    }

    #[cfg(unix)]
    #[async_trait]
    impl crate::provider::adapter::ModelAdapter for StalledAdapter {
        fn model_name(&self) -> &str {
            "stalled"
        }
        fn provider_name(&self) -> &str {
            "mock-provider"
        }
        async fn stream(
            &self,
            _request: crate::provider::ModelCompletionRequest,
        ) -> Result<crate::provider::adapter::ModelEventStream> {
            self.requested.notify_one();
            Ok(Box::pin(futures::stream::pending()))
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn an_interrupted_turn_reports_cancelled_once_when_its_cancellation_is_recorded() {
        let dir = rho_test_support::TestDir::new();
        let model = Arc::new(StalledAdapter::default());
        let engine = engine_with_turn_end_log(&dir, model.clone(), 5);
        tokio::select! {
            result = engine.run_turn(
                crate::engine::runner::turn::types::TurnRequest::new("go"),
                Arc::new(crate::engine::eval::presenter::NoopPresenter),
            ) => panic!("the stalled turn ended: {result:?}"),
            () = model.requested.notified() => {}
        }

        engine.record_cancellation("operator interrupt").await.unwrap();
        engine.record_cancellation("operator interrupt").await.unwrap();

        let payloads = turn_end_payloads(&dir);
        assert_eq!(payloads.len(), 1, "{payloads:?}");
        assert_eq!(payloads[0]["status"], "cancelled");
    }

    #[tokio::test]
    async fn finish_turn_emits_telemetry_only_when_usage_reported_and_forwards_cache_miss() {
        let dir = rho_test_support::TestDir::new();
        let engine = mock_engine(
            Arc::new(MockAdapter::default()),
            MockEngineConfig::new(&dir, rho_harness_core::config::Config::default()),
        );
        let presenter = Arc::new(TelemetryCapturingPresenter::default());
        let presenter_dyn: Arc<dyn Presenter> = presenter.clone();
        let lifecycle = engine.lifecycle_hook(presenter_dyn);

        // Turn 0: unreported usage -> no telemetry emitted.
        let _ = engine
            .finish_turn(
                TurnArtifacts {
                    response: PromptResponse::new("no-usage", StructuralUsage::default()),
                    tool_calls_count: 0,
                    completed_tools: Vec::new(),
                    generation_elapsed_ms: 500,
                    turn_started_at: std::time::Instant::now(),
                    ttft_ms: None,
                    cache_miss_tokens: None,
                    content_filtered: false,
                },
                &lifecycle,
            )
            .await
            .unwrap();
        assert!(presenter.take().is_empty());

        // Turn 1: warm cache established (cache_read = 4096).
        let u1 = usage_with(100, 500, Some(4096), None);
        let _ = engine
            .finish_turn(
                TurnArtifacts {
                    response: PromptResponse::new("ok", u1).with_completion_calls(vec![CompletionCall::new(0, u1)]),
                    tool_calls_count: 0,
                    completed_tools: Vec::new(),
                    generation_elapsed_ms: 2000,
                    turn_started_at: std::time::Instant::now(),
                    ttft_ms: Some(120),
                    cache_miss_tokens: None,
                    content_filtered: false,
                },
                &lifecycle,
            )
            .await
            .unwrap();
        let events1 = presenter.take();
        assert_eq!(events1.len(), 1);
        assert_eq!(events1[0].request_duration_ms, 2000);
        assert_eq!(events1[0].ttft_ms, Some(120));
        assert_eq!(events1[0].tokens_per_second, Some(250.0));
        assert_eq!(events1[0].cache_read_tokens, 4096);
        assert_eq!(events1[0].cache_miss_tokens, None);

        // Turn 2: warm cache invalidated (cache_read = 0, cache_write = 3000, input = 500).
        let u2 = usage_with(500, 10, Some(0), Some(3000));
        let _ = engine
            .finish_turn(
                TurnArtifacts {
                    response: PromptResponse::new("miss", u2).with_completion_calls(vec![CompletionCall::new(0, u2)]),
                    tool_calls_count: 0,
                    completed_tools: Vec::new(),
                    generation_elapsed_ms: 50,
                    turn_started_at: std::time::Instant::now(),
                    ttft_ms: Some(20),
                    cache_miss_tokens: Some(3500),
                    content_filtered: false,
                },
                &lifecycle,
            )
            .await
            .unwrap();
        let events2 = presenter.take();
        assert_eq!(events2.len(), 1);
        assert_eq!(events2[0].tokens_per_second, None);
        assert_eq!(events2[0].cache_miss_tokens, Some(3500));
    }
}
