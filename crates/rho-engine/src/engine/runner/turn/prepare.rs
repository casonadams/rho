use std::sync::Arc;

use crate::engine::AgentEngine;
use crate::engine::compactor::TurnCompaction;
use crate::engine::metrics::PruneDecision;
use crate::engine::metrics::tracker::{PromptCacheKey, SentRequest};
use crate::engine::runner::history::continuation_history;
use crate::engine::runner::sink::{TerminalApprovalSink, TerminalSinkConfig};
use crate::engine::tracking::CompactionTiming;
use rho_harness_core::error::{AppError, Result};
use rho_harness_core::model::ChatMessage;
use rho_harness_core::presentation::presenter::Presenter;
use rho_harness_core::provider::ProviderId;
use rho_harness_core::session::SessionEventKind;
use rho_harness_core::thinking::ThinkingLevel;

use super::prune::PromptCache;
use super::runner::AgentRunner;
use super::types::TurnRequest;

/// Time allowed between deciding the cache is warm and the next request reaching the provider.
const CACHE_EXPIRY_MARGIN: std::time::Duration = std::time::Duration::from_secs(30);

pub(super) struct TurnRunnerParams<'a> {
    pub project: Arc<crate::engine::context::ProjectContext>,
    pub prompt: &'a str,
    pub preamble: &'a str,
    pub budget: usize,
    pub thinking_level: Option<ThinkingLevel>,
    pub checkpoint: Option<&'a [ChatMessage]>,
    pub history: &'a [ChatMessage],
    pub compaction_timing: CompactionTiming,
}

pub(super) struct PreparedTurn {
    pub preamble: String,
    pub sink: Arc<TerminalApprovalSink>,
    pub loop_state: TurnLoopState,
}

pub(in crate::engine) struct LoadedHistory {
    pub messages: Vec<ChatMessage>,
    /// Pruned or compacted since the last request, so that request's reported usage no longer
    /// measures it.
    pub outdates_usage: bool,
    pub decision: PruneDecision,
}

/// A turn's prompt as typed and as sent, and the tokens it and the preamble add to the history.
struct TurnPrompt<'a> {
    typed: &'a str,
    sent: String,
    added_tokens: usize,
}

/// The system prompt a turn is sent with, and the project context it was built from.
struct TurnContext {
    preamble: String,
    project: Arc<crate::engine::context::ProjectContext>,
}

pub(super) struct TurnLoopState {
    /// Loaded once when the turn is prepared and reused by every step of the turn.
    pub project: Arc<crate::engine::context::ProjectContext>,
    pub visible_history: Vec<ChatMessage>,
    pub checkpoint: Option<Vec<ChatMessage>>,
    pub current_prompt: String,
    pub current_budget: usize,
    pub thinking_level: Option<ThinkingLevel>,
    pub compaction: TurnCompaction,
    pub turn_started_at: std::time::Instant,
}

impl AgentEngine {
    fn start_turn_metrics(&self, additional_tokens: usize, history: &[ChatMessage], outdates_usage: bool) {
        self.run_tracker.start(&self.config.model, self.prompt_cache_key());
        let anchor = if !history.is_empty() && !outdates_usage {
            self.usage
                .latest()
                .filter(|u| u.is_reported())
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
        TerminalApprovalSink::new(
            presenter,
            TerminalSinkConfig {
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
        let (pruned, decision) = crate::locking::lock(&self.prune_memory).prune(&raw, &sizes, &policy, cache);
        Ok(match pruned {
            Some(pruned) => LoadedHistory {
                messages: pruned,
                outdates_usage: true,
                decision,
            },
            None => LoadedHistory {
                messages: raw,
                outdates_usage: false,
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

    /// Compacts the session on rho's own initiative, with a notice when it folded something; true
    /// when it did.
    pub(super) async fn auto_compact(&self, presenter: &dyn Presenter) -> Result<bool> {
        let _spinner = presenter.start_spinner("Compacting...");
        let stats = self.compact_session(None).await?;
        let folded = !stats.summary.is_empty();
        if folded {
            presenter.print_notice(&stats.auto_notice());
        }
        Ok(folded)
    }

    /// Compacts the session when `history` plus `additional_tokens` passes the compaction threshold;
    /// true when compaction ran, which may change the session's history even if nothing folded.
    pub(in crate::engine) async fn compact_if_full(
        &self,
        presenter: &dyn Presenter,
        history: &[ChatMessage],
        additional_tokens: usize,
        outdates_usage: bool,
    ) -> Result<bool> {
        let reported = self.usage.latest().filter(|_| !outdates_usage);
        let history_tokens = reported
            .map(|u| self.consumed_context(&u) as usize)
            .unwrap_or_else(|| self.context.estimate_messages_tokens(history, &self.config.model));
        let total_tokens = history_tokens.saturating_add(additional_tokens);
        if total_tokens <= self.compaction_at() {
            return Ok(false);
        }
        self.auto_compact(presenter).await?;
        Ok(true)
    }

    /// The history the turn starts from, compacted first when it and the turn's `added_tokens`
    /// would pass the compaction threshold.
    async fn load_turn_history(&self, presenter: &dyn Presenter, added_tokens: usize) -> Result<LoadedHistory> {
        let loaded = self.load_initial_history().await?;
        let compacted = self
            .compact_if_full(presenter, &loaded.messages, added_tokens, loaded.outdates_usage)
            .await?;
        if !compacted {
            return Ok(loaded);
        }
        Ok(LoadedHistory {
            outdates_usage: true,
            ..self.load_initial_history().await?
        })
    }

    async fn build_ready_turn(
        &self,
        prompt: TurnPrompt<'_>,
        context: TurnContext,
        loaded: LoadedHistory,
        presenter: &Arc<dyn Presenter>,
        turn_started_at: std::time::Instant,
    ) -> Result<PreparedTurn> {
        let TurnContext { preamble, project } = context;
        let LoadedHistory {
            messages: history,
            outdates_usage,
            decision,
        } = loaded;
        self.session_manager
            .append_event(
                SessionEventKind::UserMessage,
                serde_json::json!({ "prompt": prompt.typed }),
            )
            .await?;
        let checkpoint = self.session_manager.load_checkpoint().await?;
        self.start_turn_metrics(prompt.added_tokens, &history, outdates_usage);
        self.run_tracker.pruned(decision);
        let sink = self.create_approval_sink(presenter);
        let loop_state = TurnLoopState {
            project,
            visible_history: history,
            checkpoint,
            current_prompt: prompt.sent,
            current_budget: self.config.max_turns,
            thinking_level: self.resolve_turn_thinking_level(prompt.typed, presenter.as_ref()).await,
            compaction: TurnCompaction::default(),
            turn_started_at,
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
        turn_started_at: std::time::Instant,
    ) -> Result<PreparedTurn> {
        self.require_model()?;
        let project = Arc::new(self.project_context().await?);
        let mut preamble = project.build_system_prompt();
        crate::mcp::append_server_instructions(&mut preamble, &self.mcp.instructions);
        let notes = self.turn_notes.pending();
        let sent = crate::engine::context::format_turn_prompt(
            &crate::engine::notes::with_notes(&notes, prompt),
            project.git_status.as_deref(),
        );
        let added_tokens =
            rho_harness_core::tokens::estimate_text_tokens(&preamble, &self.config.model).saturating_add(
                rho_harness_core::tokens::estimate_text_tokens(&sent, &self.config.model),
            );
        let loaded = self.load_turn_history(presenter.as_ref(), added_tokens).await?;
        let turn_prompt = TurnPrompt {
            typed: prompt,
            sent,
            added_tokens,
        };
        let ready = self
            .build_ready_turn(
                turn_prompt,
                TurnContext { preamble, project },
                loaded,
                presenter,
                turn_started_at,
            )
            .await?;
        self.turn_notes.consume(notes.len());
        Ok(ready)
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
            .conversation_id(self.session_manager.session_id.clone())
            .steering(request.steering.clone())
            .inbox(Some(self.inbox()))
            .monitor(Some(self.agent_monitor.clone()))
            .project_context(Some(params.project))
            .worktrees(self.worktrees.clone())
            .context_budget(Some(
                self.context_policy()
                    .session_budget(&self.config.route(), &self.context)
                    .with_compaction_timing(params.compaction_timing),
            ))
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
        let permission_hook = self.permission_hook(cwd, presenter).await;

        let runner = self
            .build_turn_runner(
                TurnRunnerParams {
                    project: Arc::clone(&loop_state.project),
                    prompt: &loop_state.current_prompt,
                    preamble,
                    budget: loop_state.current_budget,
                    thinking_level: loop_state.thinking_level,
                    checkpoint: loop_state.checkpoint.as_deref(),
                    history: &loop_state.visible_history,
                    compaction_timing: loop_state.compaction.timing(),
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
    use std::time::{Duration, Instant};

    use rho_harness_core::config::CacheRetention::{self, Auto, Long, Short};
    use rho_harness_core::config::Config;
    use rho_harness_core::model::ChatMessage;
    use test_case::test_case;

    use crate::engine::AgentEngine;
    use crate::engine::eval::mock::{MockEngineConfig, mock_engine};
    use crate::engine::metrics::StructuralUsage;
    use crate::engine::runner::turn::prune::PromptCache;
    use crate::engine::tracking::TurnUsage;
    use crate::provider::adapter::MockAdapter;

    fn cache_test_engine(
        dir: &std::path::Path,
        retention: rho_harness_core::config::CacheRetention,
        provider: &str,
    ) -> AgentEngine {
        mock_engine(
            Arc::new(MockAdapter::new("mock-model", provider)),
            MockEngineConfig::new(
                dir,
                Config {
                    cache_retention: retention,
                    auth_file: dir.join("auth.json"),
                    ..Config::default()
                },
            ),
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

    fn ago(secs: u64) -> Option<Instant> {
        Instant::now().checked_sub(Duration::from_secs(secs))
    }

    #[test_case(Short, "claude", None, false, PromptCache::Cold; "short never sent")]
    #[test_case(Short, "claude", Some(Instant::now()), false, PromptCache::Warm; "short just sent")]
    #[test_case(Short, "claude", Some(Instant::now()), true, PromptCache::Cold; "short model switched")]
    #[test_case(Short, "claude", ago(600), false, PromptCache::Cold; "short expired")]
    #[test_case(Long, "anthropic", ago(600), false, PromptCache::Warm; "long still live")]
    #[test_case(Auto, "claude", ago(600), false, PromptCache::Warm; "auto claude still live")]
    #[test_case(Auto, "anthropic", ago(600), false, PromptCache::Cold; "auto anthropic expired")]
    fn prompt_cache_is_warm_only_for_the_same_model_within_the_configured_retention(
        retention: CacheRetention,
        provider: &str,
        sent: Option<Instant>,
        switch: bool,
        expected: PromptCache,
    ) {
        let dir = crate::test_support::TestDir::new();
        let mut engine = cache_test_engine(&dir, retention, provider);
        send_then_maybe_switch(&mut engine, sent, switch);
        assert_eq!(engine.prompt_cache(), expected);
    }

    fn cached() -> StructuralUsage {
        StructuralUsage {
            input_tokens: 10,
            output_tokens: 200,
            total_tokens: 124_210,
            cached_input_tokens: Some(120_000),
            cache_creation_input_tokens: Some(4_000),
            ..StructuralUsage::default()
        }
    }

    fn uncached() -> StructuralUsage {
        StructuralUsage {
            input_tokens: 10,
            ..StructuralUsage::default()
        }
    }

    #[test_case(None, Some(cached()), true, None; "never sent")]
    #[test_case(Some(Instant::now()), None, true, None; "no usage")]
    #[test_case(Some(Instant::now()), Some(uncached()), true, None; "nothing cached")]
    #[test_case(ago(3_700), Some(cached()), true, None; "cache expired")]
    #[test_case(Some(Instant::now()), Some(cached()), false, None; "same model")]
    #[test_case(Some(Instant::now()), Some(cached()), true, Some("about 124k tokens"); "live cache switched")]
    fn cache_reset_warning_is_shown_once_for_a_model_change_over_a_live_cache(
        sent: Option<Instant>,
        usage: Option<StructuralUsage>,
        switch: bool,
        expected: Option<&str>,
    ) {
        let dir = crate::test_support::TestDir::new();
        let mut engine = cache_test_engine(&dir, Auto, "claude");
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
            None => assert_eq!(warning, None),
        }
    }

    #[tokio::test]
    async fn an_unbuildable_guard_is_reported_and_the_turn_runs_without_it() {
        use crate::test_support::CapturingPresenter;
        use rho_harness_core::presentation::presenter::Presenter;

        let dir = crate::test_support::TestDir::new();
        let mut app_config = Config {
            auth_file: dir.join("auth.json"),
            ..Config::default()
        };
        app_config.permission.enabled = true;
        app_config
            .model_roles
            .insert("guard".to_string(), "mystery/model".to_string());
        let engine = mock_engine(
            Arc::new(MockAdapter::new("mock-model", "claude")),
            MockEngineConfig::new(&dir, app_config),
        );
        let capturing = Arc::new(CapturingPresenter::default());
        let presenter: Arc<dyn Presenter> = capturing.clone();

        let hook = engine.permission_hook(dir.to_path_buf(), &presenter).await;

        assert!(hook.is_some());
        let notices = capturing.notices.lock().unwrap();
        assert!(
            notices
                .iter()
                .any(|n| n.contains("Guard model 'mystery/model' unavailable")),
            "{notices:?}"
        );
    }

    #[tokio::test]
    async fn prepared_turn_keeps_the_project_context_it_loaded() {
        use crate::test_support::CapturingPresenter;
        use rho_harness_core::presentation::presenter::Presenter;

        let dir = crate::test_support::TestDir::new();
        let app_config = Config {
            auth_file: dir.join("auth.json"),
            ..Config::default()
        };
        let engine = mock_engine(
            Arc::new(MockAdapter::text("response")),
            MockEngineConfig::new(&dir, app_config),
        );
        engine.project_context().await.unwrap();
        engine.project_context.lock().await.as_mut().unwrap().1.git_status = Some("cached".to_string());
        let presenter: Arc<dyn Presenter> = Arc::new(CapturingPresenter::default());

        let prepared = engine
            .prepare_turn("hi", &presenter, std::time::Instant::now())
            .await
            .unwrap();

        assert_eq!(prepared.loop_state.project.git_status.as_deref(), Some("cached"));
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
            MockEngineConfig::new(&dir, app_config),
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
