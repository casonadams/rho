use std::sync::Arc;

use crate::engine::AgentEngine;
use crate::engine::metrics::PruneDecision;
use crate::engine::metrics::tracker::{PromptCacheKey, SentRequest};
use crate::engine::runner::history::continuation_history;
use crate::engine::runner::sink::{TerminalApprovalSink, TerminalSinkConfig};
use rho_harness_core::error::{AppError, Result};
use rho_harness_core::model::ChatMessage;
use rho_harness_core::presentation::presenter::Presenter;
use rho_harness_core::provider::ProviderId;
use rho_harness_core::session::SessionEventKind;
use rho_harness_core::thinking::ThinkingLevel;

use super::prune::PromptCache;
use super::runner::AgentRunner;
use super::types::{TurnOutput, TurnRequest};

/// Time allowed between deciding the cache is warm and the next request reaching the provider.
const CACHE_EXPIRY_MARGIN: std::time::Duration = std::time::Duration::from_secs(30);

pub(super) enum PreparedTurnOutcome {
    Compacted(Box<TurnOutput>),
    Ready(PreparedTurn),
}

pub(super) struct TurnRunnerParams<'a> {
    pub prompt: &'a str,
    pub preamble: &'a str,
    pub budget: usize,
    pub thinking_level: Option<ThinkingLevel>,
    pub checkpoint: Option<&'a [ChatMessage]>,
    pub history: &'a [ChatMessage],
}

pub(super) struct PreparedTurn {
    pub preamble: String,
    pub sink: Arc<TerminalApprovalSink>,
    pub loop_state: TurnLoopState,
}

pub(in crate::engine) struct LoadedHistory {
    pub messages: Vec<ChatMessage>,
    pub pruned: bool,
    pub decision: PruneDecision,
}

pub(super) struct TurnLoopState {
    pub visible_history: Vec<ChatMessage>,
    pub checkpoint: Option<Vec<ChatMessage>>,
    pub current_prompt: String,
    pub current_budget: usize,
    pub thinking_level: Option<ThinkingLevel>,
    pub overflow_recovered: bool,
    pub turn_started_at: std::time::Instant,
}

impl AgentEngine {
    fn start_turn_metrics(&self, additional_tokens: usize, history: &[ChatMessage], history_pruned: bool) {
        self.run_tracker.start(&self.config.model, self.prompt_cache_key());
        let anchor = if !history.is_empty() && !history_pruned {
            self.usage
                .latest()
                .filter(|u| u.has_values())
                .map(|u| (history.len() - 1, self.consumed_context(&u) as usize))
                .filter(|&(_, tokens)| tokens > 0)
        } else {
            None
        };
        let hist_tokens = self
            .context
            .calculate_context_tokens(history, anchor, &self.config.model)
            .total_tokens;
        let est = additional_tokens.saturating_add(hist_tokens) as u64;
        self.usage.start_turn(Some(est));
    }

    pub fn create_approval_sink(&self, presenter: &Arc<dyn Presenter>) -> Arc<TerminalApprovalSink> {
        let model_label = format!("{}:{}", self.config.model, self.context_usage_display());
        TerminalApprovalSink::new(
            presenter,
            TerminalSinkConfig {
                model_label,
                run_tracker: self.run_tracker.clone(),
            },
            self.session_manager.clone(),
        )
    }

    pub(in crate::engine) async fn load_initial_history(&self) -> Result<LoadedHistory> {
        let raw = self
            .session_manager
            .active_messages()
            .await
            .map_err(|e| AppError::Session(format!("Model-visible session history could not be loaded: {e}")))?;
        let window = self.context_limit();
        let policy = super::prune::PrunePolicy::for_context_window(window).with_model(&self.config.model);
        let sizes = self.context.estimate_each_message_tokens(&raw, &self.config.model);
        let cache = self.prompt_cache();
        let (pruned, decision) = crate::engine::locking::lock(&self.prune_memory).prune(&raw, &sizes, &policy, cache);
        Ok(match pruned {
            Some(pruned) => LoadedHistory {
                messages: pruned,
                pruned: true,
                decision,
            },
            None => LoadedHistory {
                messages: raw,
                pruned: false,
                decision,
            },
        })
    }

    /// Notice for the first model or thinking change since the last request while its prompt cache is alive:
    /// the next request rewrites the cache.
    pub fn cache_reset_warning(&self) -> Option<String> {
        let sent = self.live_prompt_cache()?;
        if sent.cache_key == self.prompt_cache_key() {
            return None;
        }
        let usage = self.usage.latest()?;
        let cached = usage.cached_input_tokens.unwrap_or(0) + usage.cache_creation_input_tokens.unwrap_or(0);
        (cached > 0 && self.run_tracker.note_cache_reset()).then(|| {
            format!(
                "The next request rewrites the prompt cache (about {} tokens at the cache-write price).",
                crate::engine::metrics::types::format_tokens(cached)
            )
        })
    }

    fn prompt_cache_key(&self) -> PromptCacheKey {
        self.model
            .as_ref()
            .map(|model| PromptCacheKey {
                provider: model.provider_name().to_string(),
                model: model.model_name().to_string(),
                settings: model.cache_settings().map(String::from),
            })
            .unwrap_or_default()
    }

    /// The last request, while the prompt cache it wrote is still alive.
    fn live_prompt_cache(&self) -> Option<SentRequest> {
        let sent = self.run_tracker.last_request()?;
        let subscription = sent.cache_key.provider == ProviderId::ClaudeCode.as_str();
        let lifetime = self
            .config
            .cache_retention
            .ttl(subscription)
            .saturating_sub(CACHE_EXPIRY_MARGIN);
        (sent.started.elapsed() < lifetime).then_some(sent)
    }

    fn prompt_cache(&self) -> PromptCache {
        match self.live_prompt_cache() {
            Some(sent) if sent.cache_key == self.prompt_cache_key() => PromptCache::Warm,
            _ => PromptCache::Cold,
        }
    }

    pub(crate) async fn check_proactive_compaction(
        &self,
        presenter: &dyn Presenter,
        history: &mut Vec<ChatMessage>,
        additional_tokens: usize,
        history_pruned: bool,
    ) -> Result<Option<crate::engine::CompactionStats>> {
        let reported = self.usage.latest().filter(|_| !history_pruned);
        let history_tokens = reported
            .map(|u| self.consumed_context(&u) as usize)
            .unwrap_or_else(|| self.context.estimate_messages_tokens(history, &self.config.model));
        let total_tokens = history_tokens.saturating_add(additional_tokens);
        let limit = self.context_limit();
        let threshold = limit.saturating_sub(self.config.reserve_tokens);
        if total_tokens > threshold {
            let _spinner = presenter.start_spinner("Compacting...");
            let stats = self.compact_session(None).await?;
            *history = self.session_manager.active_messages().await?;
            presenter.print_notice(&format!(
                "[Auto-compacted context: {} -> {} tokens (saved {})]",
                stats.tokens_before, stats.tokens_after, stats.saved_tokens
            ));
            Ok(Some(stats))
        } else {
            Ok(None)
        }
    }

    async fn check_turn_proactive_compaction(
        &self,
        preamble: &str,
        prompt: &str,
        presenter: &dyn Presenter,
        history: &mut Vec<ChatMessage>,
        history_pruned: bool,
    ) -> Result<Option<TurnOutput>> {
        let add_tokens = rho_harness_core::tokens::estimate_text_tokens(preamble, &self.config.model).saturating_add(
            rho_harness_core::tokens::estimate_text_tokens(prompt, &self.config.model),
        );
        if self
            .check_proactive_compaction(presenter, history, add_tokens, history_pruned)
            .await?
            .is_some()
        {
            return self.compacted_turn_output(super::RunStatus::Compacted).await.map(Some);
        }
        Ok(None)
    }

    async fn build_ready_turn(
        &self,
        user_prompt: &str,
        effective_prompt: &str,
        preamble: String,
        loaded: LoadedHistory,
        presenter: &Arc<dyn Presenter>,
    ) -> Result<PreparedTurn> {
        let LoadedHistory {
            messages: history,
            pruned: history_pruned,
            decision,
        } = loaded;
        self.session_manager
            .append_event(
                SessionEventKind::UserMessage,
                serde_json::json!({ "prompt": user_prompt }),
            )
            .await?;
        let checkpoint = self.session_manager.load_checkpoint().await?;
        let add_tokens = rho_harness_core::tokens::estimate_text_tokens(&preamble, &self.config.model).saturating_add(
            rho_harness_core::tokens::estimate_text_tokens(effective_prompt, &self.config.model),
        );
        self.start_turn_metrics(add_tokens, &history, history_pruned);
        self.run_tracker.pruned(decision);
        let sink = self.create_approval_sink(presenter);
        let loop_state = TurnLoopState {
            visible_history: history,
            checkpoint,
            current_prompt: effective_prompt.to_string(),
            current_budget: self.config.max_turns,
            thinking_level: None,
            overflow_recovered: false,
            turn_started_at: std::time::Instant::now(),
        };
        Ok(PreparedTurn {
            preamble,
            sink,
            loop_state,
        })
    }

    pub(super) async fn prepare_turn(
        &self,
        prompt: &str,
        presenter: &Arc<dyn Presenter>,
    ) -> Result<PreparedTurnOutcome> {
        self.require_model()?;
        let context = self.project_context().await?;
        let mut preamble = context.build_system_prompt();
        crate::mcp::append_server_instructions(&mut preamble, &self.mcp.instructions);
        let notes = self.turn_notes.pending();
        let effective_prompt = crate::engine::context::format_turn_prompt(
            &crate::engine::notes::with_notes(&notes, prompt),
            context.git_status.as_deref(),
        );
        let mut loaded = self.load_initial_history().await?;
        if let Some(out) = self
            .check_turn_proactive_compaction(
                &preamble,
                &effective_prompt,
                presenter.as_ref(),
                &mut loaded.messages,
                loaded.pruned,
            )
            .await?
        {
            return Ok(PreparedTurnOutcome::Compacted(Box::new(out)));
        }
        let mut ready = self
            .build_ready_turn(prompt, &effective_prompt, preamble, loaded, presenter)
            .await?;
        ready.loop_state.thinking_level = self.resolve_turn_thinking_level(prompt).await;
        self.turn_notes.consume(notes.len());
        Ok(PreparedTurnOutcome::Ready(ready))
    }

    pub(in crate::engine) fn require_model(&self) -> Result<crate::engine::compactor::llm::ModelHandle> {
        self.model.clone().ok_or_else(|| {
            rho_harness_core::error::AppError::Config(
                "No model selected. Pick one with /model, `rho --model <provider>/<model>`, or save a default with `rho config model <provider>/<model>`."
                    .to_string(),
            )
        })
    }

    async fn build_turn_runner<'a>(
        &self,
        params: TurnRunnerParams<'a>,
        sink: Arc<TerminalApprovalSink>,
        request: &TurnRequest<'_>,
    ) -> Result<AgentRunner> {
        let tools = self.tools.read().await.clone();
        let model = self.require_model()?;
        let mut runner = AgentRunner::new(model, tools, self.session_manager.clone(), sink)
            .preamble(params.preamble)
            .prompt(params.prompt)
            .budget(params.budget)
            .thinking_level(params.thinking_level)
            .steering(request.steering.clone())
            .inbox(Some(self.inbox()))
            .monitor(Some(self.agent_monitor.clone()))
            .project_context(self.project_context().await.ok().map(Arc::new))
            .worktrees(self.worktrees.clone())
            .context_budget(Some(self.context.budget(
                self.context_limit(),
                self.context_limit_source(),
                self.context_limit().saturating_sub(self.config.reserve_tokens),
            )))
            .working_dir(self.base_dir.clone())
            .a2ui_config(self.config.ui.a2ui_widgets, self.config.ui.a2ui.clone());

        if let Some(pending) = params.checkpoint {
            let combined = continuation_history(params.history, pending);
            runner = runner.history(combined);
        } else {
            runner = runner.history(params.history.to_vec());
        }
        Ok(runner)
    }

    pub(super) fn lifecycle_hook(&self, presenter: Arc<dyn Presenter>) -> Arc<crate::hook::LifecycleHook> {
        Arc::new(
            crate::hook::LifecycleHook::new(
                self.base_dir.clone(),
                self.session_manager.session_id.clone(),
                presenter,
            )
            .with_hook_dirs(self.config.hook_dirs.clone()),
        )
    }

    /// The permission hook for a turn; a guard model that cannot be built is reported and the turn
    /// runs without it.
    async fn permission_hook(
        &self,
        cwd: std::path::PathBuf,
        presenter: &Arc<dyn Presenter>,
    ) -> Option<Arc<crate::permission::PermissionHook>> {
        if !self.config.permission.enabled {
            return None;
        }
        let mcp_servers = if self.config.mcp.enabled {
            self.config
                .mcp
                .servers
                .keys()
                .map(|name| crate::mcp::naming::sanitize_segment(name))
                .collect()
        } else {
            Vec::new()
        };
        let mut hook = crate::permission::PermissionHook::new(Some(cwd), presenter.clone())
            .with_mcp_servers(mcp_servers)
            .with_guard_all_tools(self.config.permission.guard_all_tools);
        match self.router.build_guard_decision(&self.config).await {
            Ok(Some(guard)) => {
                hook = hook.with_guard(
                    crate::permission::guard::GuardEvaluator::from_decision(guard)
                        .with_audit_log(self.config.permission.guard_audit_log),
                );
            }
            Ok(None) => {}
            Err(err) => presenter.print_notice(&format!(
                "[Guard model '{}' unavailable, running without it: {err}]",
                self.config.guard_model().unwrap_or_default()
            )),
        }
        Some(Arc::new(hook))
    }

    pub(super) async fn prepare_step_runner<'a>(
        &self,
        sink: &Arc<TerminalApprovalSink>,
        preamble: &'a str,
        request: &TurnRequest<'_>,
        lifecycle_hook: &Arc<crate::hook::LifecycleHook>,
        loop_state: &'a TurnLoopState,
    ) -> Result<AgentRunner> {
        let presenter = lifecycle_hook.presenter();
        let cwd = self.base_dir.clone();
        lifecycle_hook.notify_turn_start(&loop_state.current_prompt).await;

        let permission_hook = self.permission_hook(cwd, presenter).await;

        let runner = self
            .build_turn_runner(
                TurnRunnerParams {
                    prompt: &loop_state.current_prompt,
                    preamble,
                    budget: loop_state.current_budget,
                    thinking_level: loop_state.thinking_level,
                    checkpoint: loop_state.checkpoint.as_deref(),
                    history: &loop_state.visible_history,
                },
                sink.clone(),
                request,
            )
            .await?
            .lifecycle(Some(lifecycle_hook.clone()))
            .permission(permission_hook)
            .cancellation(request.cancellation.cloned());

        Ok(runner)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use rho_harness_core::config::Config;
    use rho_harness_core::model::ChatMessage;

    use crate::engine::AgentEngine;
    use crate::engine::eval::mock::{MockEngineConfig, mock_engine};
    use crate::engine::metrics::StructuralUsage;
    use crate::engine::tracking::TurnUsage;
    use crate::provider::adapter::MockAdapter;

    fn cache_test_engine(
        dir: &std::path::Path,
        retention: rho_harness_core::config::CacheRetention,
        provider: &str,
    ) -> AgentEngine {
        mock_engine(
            Arc::new(MockAdapter::new("mock-model", provider)),
            MockEngineConfig {
                base_dir: dir,
                app_config: Config {
                    cache_retention: retention,
                    auth_file: dir.join("auth.json"),
                    ..Config::default()
                },
                session_manager: None,
                built_in_tools: None,
            },
        )
    }

    /// Records a request sent at `sent` and, when `switch` is set, then moves the engine to another model.
    fn send_then_maybe_switch(engine: &mut AgentEngine, sent: Option<std::time::Instant>, switch: bool) {
        use crate::engine::metrics::types::{CompletionCall, StructuralUsage};
        if let Some(sent) = sent {
            engine.run_tracker.start("mock-model", engine.prompt_cache_key());
            engine
                .run_tracker
                .completion(CompletionCall::new(0, StructuralUsage::default()), sent);
        }
        if switch {
            let provider = engine.model.as_ref().unwrap().provider_name().to_string();
            engine.model = Some(Arc::new(MockAdapter::new("other-model", provider)));
        }
    }

    #[test]
    fn prompt_cache_is_warm_only_for_the_same_model_within_the_configured_retention() {
        use crate::engine::runner::turn::prune::PromptCache;
        use rho_harness_core::config::CacheRetention::{Auto, Long, Short};
        use std::time::{Duration, Instant};

        let dir = tempfile::tempdir().unwrap();
        let now = Some(Instant::now());
        let ten_minutes_ago = Instant::now().checked_sub(Duration::from_secs(600));
        let cases = [
            (Short, "claude", None, false, PromptCache::Cold),
            (Short, "claude", now, false, PromptCache::Warm),
            (Short, "claude", now, true, PromptCache::Cold),
            (Short, "claude", ten_minutes_ago, false, PromptCache::Cold),
            (Long, "anthropic", ten_minutes_ago, false, PromptCache::Warm),
            (Auto, "claude", ten_minutes_ago, false, PromptCache::Warm),
            (Auto, "anthropic", ten_minutes_ago, false, PromptCache::Cold),
        ];
        for (retention, provider, sent, switch, expected) in cases {
            let mut engine = cache_test_engine(dir.path(), retention, provider);
            send_then_maybe_switch(&mut engine, sent, switch);
            assert_eq!(
                engine.prompt_cache(),
                expected,
                "{retention} {provider} {sent:?} {switch}"
            );
        }
    }

    #[test]
    fn cache_reset_warning_is_shown_once_for_a_model_change_over_a_live_cache() {
        use crate::engine::metrics::types::StructuralUsage;
        use rho_harness_core::config::CacheRetention;
        use std::time::{Duration, Instant};

        let dir = tempfile::tempdir().unwrap();
        let cached = StructuralUsage {
            input_tokens: 10,
            output_tokens: 200,
            total_tokens: 124_210,
            cached_input_tokens: Some(120_000),
            cache_creation_input_tokens: Some(4_000),
            ..StructuralUsage::default()
        };
        let uncached = StructuralUsage {
            input_tokens: 10,
            ..StructuralUsage::default()
        };
        let now = Some(Instant::now());
        let an_hour_ago = Instant::now().checked_sub(Duration::from_secs(3_700));
        let cases = [
            (None, Some(cached), true, None),
            (now, None, true, None),
            (now, Some(uncached), true, None),
            (an_hour_ago, Some(cached), true, None),
            (now, Some(cached), false, None),
            (now, Some(cached), true, Some("about 124k tokens")),
        ];
        for (sent, usage, switch, expected) in cases {
            let mut engine = cache_test_engine(dir.path(), CacheRetention::Auto, "claude");
            if let Some(usage) = usage {
                engine.usage.record(usage);
            }
            send_then_maybe_switch(&mut engine, sent, switch);
            let warning = engine.cache_reset_warning();
            match expected {
                Some(text) => {
                    assert!(warning.as_deref().is_some_and(|w| w.contains(text)), "{warning:?}");
                    assert_eq!(engine.cache_reset_warning(), None, "shown twice");
                }
                None => assert_eq!(warning, None, "{sent:?} {usage:?} {switch}"),
            }
        }
    }

    #[tokio::test]
    async fn an_unbuildable_guard_is_reported_and_the_turn_runs_without_it() {
        use crate::test_support::CapturingPresenter;
        use rho_harness_core::presentation::presenter::Presenter;

        let dir = tempfile::tempdir().unwrap();
        let mut app_config = Config {
            auth_file: dir.path().join("auth.json"),
            ..Config::default()
        };
        app_config.permission.enabled = true;
        app_config
            .model_roles
            .insert("guard".to_string(), "mystery/model".to_string());
        let engine = mock_engine(
            Arc::new(MockAdapter::new("mock-model", "claude")),
            MockEngineConfig {
                base_dir: dir.path(),
                app_config,
                session_manager: None,
                built_in_tools: None,
            },
        );
        let capturing = Arc::new(CapturingPresenter::default());
        let presenter: Arc<dyn Presenter> = capturing.clone();

        let hook = engine.permission_hook(dir.path().to_path_buf(), &presenter).await;

        assert!(hook.is_some());
        let notices = capturing.notices.lock().unwrap();
        assert!(
            notices
                .iter()
                .any(|n| n.contains("Guard model 'mystery/model' unavailable")),
            "{notices:?}"
        );
    }

    #[test]
    fn test_pruned_history_estimate_replaces_stale_usage_anchor() {
        let dir = crate::test_support::TestDir::new();
        let app_config = Config {
            auth_file: dir.join("auth.json"),
            ..Config::default()
        };
        let engine = mock_engine(
            Arc::new(MockAdapter::text("response")),
            MockEngineConfig {
                base_dir: &dir,
                app_config,
                session_manager: None,
                built_in_tools: None,
            },
        );
        let stale = StructuralUsage {
            input_tokens: 120_000,
            output_tokens: 0,
            total_tokens: 120_000,
            cached_input_tokens: None,
            cache_creation_input_tokens: None,
            tool_use_prompt_tokens: None,
            reasoning_tokens: None,
        };
        engine.usage.record_turn(TurnUsage::new(stale, stale), 100);
        let history = vec![ChatMessage::user("hello"), ChatMessage::assistant("hi")];

        engine.start_turn_metrics(10, &history, true);

        let displayed = engine.usage.latest().unwrap().input_tokens;
        assert!(
            displayed < 1_000,
            "displayed context stayed anchored to stale usage: {displayed}"
        );
    }
}
