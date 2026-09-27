//! Tests for the mid-run auto-compaction hook: compaction at the
//! before-next-assistant-call boundary with a history patch, and the
//! ephemeral fallback when the durable compaction cannot persist.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use rho_harness_core::config::{Config, PermissionConfig};
use rho_harness_core::presentation::activity::ActivityToken;
use rho_harness_core::presentation::presenter::Presenter;
use rho_harness_core::presentation::stream::ToolStreamPort;
use rho_harness_core::presentation::{SessionStatus, ToolLine, WelcomeDisplay};
use rho_harness_core::session::tree::TreeNodeKind;
use rig::agent::hook::CompletionCallAction;
use rig::completion::Usage;
use rig::memory::ConversationMemory;
use rig::message::Message;
use rig::message::{AssistantContent, ToolResultContent, UserContent};
use rig::test_utils::{MockCompletionModel, MockStreamEvent};
use rig::tool::{DynamicTool, ToolOutput};

use crate::engine::eval::mock::{MockEngineConfig, final_event, mock_engine};
use crate::engine::metrics::StructuralUsage;
use crate::engine::runner::TurnRequest;
use crate::engine::runner::turn::auto_compact::{AutoCompactHook, trigger_tokens};
use crate::engine::tracking::ContextTracker;

#[derive(Default)]
struct CapturingPresenter {
    notices: Mutex<Vec<String>>,
    spinner_messages: Mutex<Vec<String>>,
}

#[async_trait]
impl Presenter for CapturingPresenter {
    fn write_output(&self, _text: &str) {}
    fn print_welcome(&self, _display: &WelcomeDisplay) {}
    fn print_session_status(&self, _display: &SessionStatus) {}
    fn print_notice(&self, text: &str) {
        self.notices.lock().unwrap().push(text.to_string());
    }
    fn print_user_block(&self, _input: &str) {}
    fn print_token(&self, _token: &str) {}
    fn print_thinking_token(&self, _token: &str) {}
    fn finish_tool_line(&self, _line: ToolLine) {}
    fn flush(&self) {}
    fn has_interactive_ui(&self) -> bool {
        false
    }
    fn start_spinner(&self, message: &str) -> ActivityToken {
        self.spinner_messages.lock().unwrap().push(message.to_string());
        ActivityToken::default()
    }
    fn start_tool_spinner(&self, _name: &str, _arguments: &serde_json::Value) -> ActivityToken {
        ActivityToken::default()
    }
    fn start_tool_run(&self, _name: &str, _arguments: &serde_json::Value) {}
    fn stream_port(&self) -> ToolStreamPort {
        ToolStreamPort::default()
    }
    async fn prompt_continue_budget(&self, _max_turns: usize) -> bool {
        false
    }
}

fn echo_tool() -> DynamicTool {
    DynamicTool::new(
        "echo_tool",
        "Echoes a fixed string",
        serde_json::json!({"type": "object", "properties": {}}),
        |_ctx, _args| Box::pin(async { Ok(ToolOutput::text("echoed")) }),
    )
}

fn engine_for(dir: &std::path::Path, model: MockCompletionModel) -> crate::engine::AgentEngine {
    let app_config = Config {
        model: "mock-model".to_string(),
        keep_recent_tokens: 5,
        permission: PermissionConfig { enabled: false },
        auth_file: dir.join("auth.json"),
        ..Config::default()
    };
    mock_engine(
        model,
        MockEngineConfig {
            base_dir: dir,
            app_config,
            session_manager: None,
            built_in_tools: Some(vec![echo_tool()]),
        },
    )
}

async fn seed_history(engine: &crate::engine::AgentEngine) {
    let sid = &engine.session_manager.session_id;
    ConversationMemory::append(
        &engine.session_manager,
        sid,
        vec![Message::user("prior prompt"), Message::assistant("prior response")],
    )
    .await
    .unwrap();
}

async fn assert_compaction_node_present(engine: &crate::engine::AgentEngine) {
    let tree = engine.session_manager.load_tree().await.unwrap();
    let leaf_id = tree.active_leaf_id.as_ref().unwrap();
    assert!(
        tree.ancestor_nodes(leaf_id)
            .iter()
            .any(|n| n.kind == TreeNodeKind::Compaction)
    );
}

#[tokio::test]
async fn test_mid_run_auto_compaction_continues_run_on_compacted_context() {
    let dir = std::env::temp_dir().join(format!("midrun_compact_{}", uuid::Uuid::new_v4()));
    let turn_usage = Usage {
        input_tokens: 120_000,
        output_tokens: 10,
        total_tokens: 120_010,
        ..Default::default()
    };
    let model = MockCompletionModel::from_stream_turns([
        [
            MockStreamEvent::tool_call("t1", "echo_tool", serde_json::json!({})),
            final_event(turn_usage),
        ],
        [MockStreamEvent::text("continued"), final_event(Usage::default())],
    ]);
    let engine = engine_for(&dir, model.clone());
    seed_history(&engine).await;

    let presenter = Arc::new(CapturingPresenter::default());
    let output = engine
        .run_turn(TurnRequest::new("run the echo tool"), presenter.clone())
        .await
        .unwrap();

    // The run continued to completion on the compacted context instead of
    // halting or waiting for the next user prompt.
    assert_eq!(output.status, crate::engine::runner::RunStatus::Completed);
    assert_eq!(output.final_text, "continued");

    // Turn 1, the compaction summarizer, then the resumed model call.
    let requests = model.requests();
    assert_eq!(requests.len(), 3);
    let patched = &requests[2].chat_history;
    // [preamble, compaction summary, kept tree tail, user prompt, assistant
    // tool call, tool result prompt]
    assert_eq!(patched.len(), 6, "summary + kept tree tail + run messages");
    match &patched[1] {
        Message::System { content } => assert!(content.contains("## Goal"), "summary: {content}"),
        other => panic!("expected compaction summary system message, got {other:?}"),
    }
    let as_text = as_text_of;
    assert_eq!(as_text(&patched[2]), "prior response");
    assert!(as_text(&patched[3]).contains("run the echo tool"));
    assert!(as_text(&patched[5]).contains("echoed"));

    assert_compaction_node_present(&engine).await;
    assert!(
        presenter
            .notices
            .lock()
            .unwrap()
            .iter()
            .any(|n| n.contains("Auto-compacted context"))
    );
    assert!(
        presenter
            .spinner_messages
            .lock()
            .unwrap()
            .iter()
            .any(|m| m == "Compacting...")
    );
}

#[tokio::test]
async fn test_mid_run_auto_compaction_falls_back_to_ephemeral_summary_with_pending_checkpoint() {
    let dir = std::env::temp_dir().join(format!("midrun_ephemeral_{}", uuid::Uuid::new_v4()));
    let model =
        MockCompletionModel::from_stream_turns([[MockStreamEvent::text("response"), final_event(Usage::default())]]);
    let engine = engine_for(&dir, model);
    seed_history(&engine).await;
    engine
        .session_manager
        .save_checkpoint(vec![Message::user("pending checkpoint work")])
        .await
        .unwrap();
    let usage = Usage {
        input_tokens: 120_000,
        ..Default::default()
    }
    .into();
    engine
        .usage
        .record_turn(crate::engine::tracking::TurnUsage::new(usage, usage), 100);

    let presenter = Arc::new(CapturingPresenter::default());
    let hook = AutoCompactHook::new(
        engine.session_compactor(),
        presenter.clone(),
        engine.usage.clone(),
        engine.context,
        &engine.config.provider,
        engine.config.reserve_tokens,
    );
    let history = vec![
        Message::user("prior prompt"),
        Message::assistant("prior response"),
        Message::user("pending checkpoint work"),
    ];
    let action = hook.handle(None, &history, &Message::user("latest prompt")).await;

    match action {
        CompletionCallAction::Patch(patch) => {
            let patched = patch.history.expect("patch supplies history");
            match &patched[0] {
                Message::System { content } => assert!(!content.trim().is_empty()),
                other => panic!("expected compaction summary system message, got {other:?}"),
            }
            // Everything from the cut on is kept verbatim, including the
            // pending checkpoint message that the durable store does not have.
            assert_eq!(patched.len(), 2);
            assert!(as_text_of(&patched[1]).contains("pending checkpoint work"));
        }
        other => panic!("expected history patch, got {other:?}"),
    }
    let tree = engine.session_manager.load_tree().await.unwrap();
    let leaf_id = tree.active_leaf_id.as_ref().unwrap();
    assert!(
        !tree
            .ancestor_nodes(leaf_id)
            .iter()
            .any(|n| n.kind == TreeNodeKind::Compaction)
    );
}

#[tokio::test]
async fn test_auto_compact_hook_patches_pruned_historical_bash_output() {
    let dir = tempfile::tempdir().unwrap();
    let model = MockCompletionModel::text("done");
    let engine = engine_for(dir.path(), model);
    let presenter = Arc::new(CapturingPresenter::default());
    let hook = AutoCompactHook::new(
        engine.session_compactor(),
        presenter,
        engine.usage.clone(),
        engine.context,
        "anthropic",
        50,
    )
    .with_prune_policy(crate::engine::runner::turn::PrunePolicy::unconstrained(1, 15));

    let output = (1..=30)
        .map(|i| format!("cargo build line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let output_with_footer = format!(
        "{output}\n\n[Command completed successfully with exit code 0 (30 lines, 600B). Full log: /tmp/log.txt]"
    );

    let call = rig::message::ToolCall::new(
        rig::message::ToolCallId::new_or_mint("c1"),
        rig::message::ToolFunction::new("bash".to_string(), serde_json::json!({ "command": "cargo build" })),
    );
    let res = rig::message::ToolResult {
        call: rig::message::ToolCallId::new_or_mint("c1"),
        provider: None,
        name: "bash".to_string(),
        content: vec![rig::message::ToolResultContent::Text(rig::message::Text::new(
            output_with_footer,
        ))],
    };

    let history = vec![
        Message::user("Please build"),
        Message::Assistant {
            id: None,
            content: vec![AssistantContent::ToolCall(call)],
        },
        Message::User {
            content: vec![UserContent::ToolResult(res)],
        },
        Message::assistant("Build completed successfully"),
        Message::user("Now run tests"),
    ];

    let action = hook.handle(None, &history, &Message::user("latest prompt")).await;
    match action {
        CompletionCallAction::Patch(patch) => {
            let patched = patch.history.expect("patch supplies history");
            assert_eq!(patched.len(), 5);
            let text = as_text_of(&patched[2]);
            assert!(text.contains("[Command 'cargo build' completed with exit code 0. Output pruned (30 lines, 600B). Full log: /tmp/log.txt]"));
        }
        other => panic!("expected history patch with pruned bash output, got {other:?}"),
    }
}

#[tokio::test]
async fn test_auto_compact_hook_preserves_recent_tool_outputs_under_default_cache_policy() {
    let dir = tempfile::tempdir().unwrap();
    let model = MockCompletionModel::text("done");
    let engine = engine_for(dir.path(), model);
    let presenter = Arc::new(CapturingPresenter::default());
    let hook = AutoCompactHook::new(
        engine.session_compactor(),
        presenter,
        engine.usage.clone(),
        engine.context,
        "anthropic",
        50,
    );

    let output = (1..=30)
        .map(|i| format!("cargo build line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let call = rig::message::ToolCall::new(
        rig::message::ToolCallId::new_or_mint("c1"),
        rig::message::ToolFunction::new("bash".to_string(), serde_json::json!({ "command": "cargo build" })),
    );
    let res = rig::message::ToolResult {
        call: rig::message::ToolCallId::new_or_mint("c1"),
        provider: None,
        name: "bash".to_string(),
        content: vec![rig::message::ToolResultContent::Text(rig::message::Text::new(output))],
    };

    let history = vec![
        Message::user("Please build"),
        Message::Assistant {
            id: None,
            content: vec![AssistantContent::ToolCall(call)],
        },
        Message::User {
            content: vec![UserContent::ToolResult(res)],
        },
        Message::assistant("Build completed"),
        Message::user("Now run tests"),
    ];

    let action = hook.handle(None, &history, &Message::user("latest prompt")).await;
    match action {
        CompletionCallAction::Continue => {}
        other => panic!("expected Continue to preserve prompt cache, got {other:?}"),
    }
}

#[derive(Default)]
struct CapturingDemotionHook {
    demotions: std::sync::Mutex<Vec<(String, Vec<Message>)>>,
    fail: bool,
    panic: bool,
}

impl rig::memory::DemotionHook for CapturingDemotionHook {
    fn on_demote<'a>(
        &'a self,
        conversation_id: &'a str,
        messages: Vec<Message>,
    ) -> rig::wasm_compat::WasmBoxedFuture<'a, Result<(), rig::memory::MemoryError>> {
        Box::pin(async move {
            if self.panic {
                panic!("simulated hook panic");
            }
            if self.fail {
                return Err(rig::memory::MemoryError::Internal("test demotion failure".to_string()));
            }
            self.demotions
                .lock()
                .unwrap()
                .push((conversation_id.to_string(), messages));
            Ok(())
        })
    }
}

#[tokio::test]
async fn test_auto_compact_hook_forwards_evicted_messages_to_demotion_hook() {
    let dir = std::env::temp_dir().join(format!("demote_eph_{}", uuid::Uuid::new_v4()));
    let model =
        MockCompletionModel::from_stream_turns([[MockStreamEvent::text("response"), final_event(Usage::default())]]);
    let engine = engine_for(&dir, model);
    seed_history(&engine).await;
    engine
        .session_manager
        .save_checkpoint(vec![Message::user("pending checkpoint work")])
        .await
        .unwrap();
    let usage = Usage {
        input_tokens: 120_000,
        ..Default::default()
    }
    .into();
    engine
        .usage
        .record_turn(crate::engine::tracking::TurnUsage::new(usage, usage), 100);

    let presenter = Arc::new(CapturingPresenter::default());
    let hook_recorder = Arc::new(CapturingDemotionHook::default());
    let hook = AutoCompactHook::new(
        engine.session_compactor(),
        presenter.clone(),
        engine.usage.clone(),
        engine.context,
        &engine.config.provider,
        engine.config.reserve_tokens,
    )
    .with_demotion_hook(hook_recorder.clone());

    let history = vec![
        Message::user("prior prompt"),
        Message::assistant("prior response"),
        Message::user("pending checkpoint work"),
    ];
    let action = hook.handle(None, &history, &Message::user("latest prompt")).await;
    assert!(matches!(action, CompletionCallAction::Patch(_)));

    let demotions = hook_recorder.demotions.lock().unwrap();
    assert_eq!(demotions.len(), 1);
    assert_eq!(demotions[0].0, engine.session_manager.session_id);
    assert!(!demotions[0].1.is_empty());
}

#[tokio::test]
async fn test_auto_compact_hook_failing_demotion_hook_does_not_abort_turn() {
    let dir = std::env::temp_dir().join(format!("demote_fail_{}", uuid::Uuid::new_v4()));
    let model =
        MockCompletionModel::from_stream_turns([[MockStreamEvent::text("response"), final_event(Usage::default())]]);
    let engine = engine_for(&dir, model);
    seed_history(&engine).await;
    engine
        .session_manager
        .save_checkpoint(vec![Message::user("pending checkpoint work")])
        .await
        .unwrap();
    let usage = Usage {
        input_tokens: 120_000,
        ..Default::default()
    }
    .into();
    engine
        .usage
        .record_turn(crate::engine::tracking::TurnUsage::new(usage, usage), 100);

    let presenter = Arc::new(CapturingPresenter::default());
    let hook_recorder = Arc::new(CapturingDemotionHook {
        fail: true,
        ..Default::default()
    });
    let hook = AutoCompactHook::new(
        engine.session_compactor(),
        presenter.clone(),
        engine.usage.clone(),
        engine.context,
        &engine.config.provider,
        engine.config.reserve_tokens,
    )
    .with_demotion_hook(hook_recorder);

    let history = vec![
        Message::user("prior prompt"),
        Message::assistant("prior response"),
        Message::user("pending checkpoint work"),
    ];
    let action = hook.handle(None, &history, &Message::user("latest prompt")).await;
    assert!(matches!(action, CompletionCallAction::Patch(_)));
}

#[tokio::test]
async fn test_auto_compact_hook_panicking_demotion_hook_does_not_abort_turn() {
    let dir = std::env::temp_dir().join(format!("demote_panic_{}", uuid::Uuid::new_v4()));
    let model =
        MockCompletionModel::from_stream_turns([[MockStreamEvent::text("response"), final_event(Usage::default())]]);
    let engine = engine_for(&dir, model);
    seed_history(&engine).await;
    engine
        .session_manager
        .save_checkpoint(vec![Message::user("pending checkpoint work")])
        .await
        .unwrap();
    let usage = Usage {
        input_tokens: 120_000,
        ..Default::default()
    }
    .into();
    engine
        .usage
        .record_turn(crate::engine::tracking::TurnUsage::new(usage, usage), 100);

    let presenter = Arc::new(CapturingPresenter::default());
    let hook_recorder = Arc::new(CapturingDemotionHook {
        panic: true,
        ..Default::default()
    });
    let hook = AutoCompactHook::new(
        engine.session_compactor(),
        presenter.clone(),
        engine.usage.clone(),
        engine.context,
        &engine.config.provider,
        engine.config.reserve_tokens,
    )
    .with_demotion_hook(hook_recorder);

    let history = vec![
        Message::user("prior prompt"),
        Message::assistant("prior response"),
        Message::user("pending checkpoint work"),
    ];
    let action = hook.handle(None, &history, &Message::user("latest prompt")).await;
    assert!(matches!(action, CompletionCallAction::Patch(_)));
}

fn as_text_of(message: &Message) -> String {
    match message {
        Message::User { content } => content
            .iter()
            .filter_map(|part| match part {
                UserContent::Text(text) => Some(text.text.clone()),
                UserContent::ToolResult(result) => Some(
                    result
                        .content
                        .iter()
                        .filter_map(ToolResultContent::as_text)
                        .collect::<Vec<_>>()
                        .join(" "),
                ),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" "),
        Message::Assistant { content, .. } => content
            .iter()
            .filter_map(|part| match part {
                AssistantContent::Text(text) => Some(text.text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" "),
        Message::System { content } => content.clone(),
    }
}

#[tokio::test]
async fn test_auto_compaction_check_with_20_messages_reuses_cached_counts() {
    let dir = tempfile::tempdir().unwrap();
    let model = MockCompletionModel::default();
    let engine = engine_for(dir.path(), model);

    let presenter = CapturingPresenter::default();
    let mut history = Vec::new();
    for i in 0..20 {
        if i % 2 == 0 {
            history.push(Message::user(format!(
                "User message {i} with some detailed context text."
            )));
        } else {
            history.push(Message::assistant(format!(
                "Assistant response {i} providing helpful explanations."
            )));
        }
    }

    // First check computes tokens for all 20 messages
    let _ = engine
        .check_proactive_compaction(&presenter, (&mut history, 0))
        .await
        .unwrap();
    let misses_after_first = engine.context.token_cache().lock().unwrap().misses();
    let hits_after_first = engine.context.token_cache().lock().unwrap().hits();
    assert_eq!(misses_after_first, 20);
    assert_eq!(hits_after_first, 0);

    // Second check with identical history reuses memoized counts
    let _ = engine
        .check_proactive_compaction(&presenter, (&mut history, 0))
        .await
        .unwrap();
    let misses_after_second = engine.context.token_cache().lock().unwrap().misses();
    let hits_after_second = engine.context.token_cache().lock().unwrap().hits();
    assert_eq!(misses_after_second, 20);
    assert_eq!(hits_after_second, 20);
}

#[test]
fn test_trigger_tokens_reconciles_anchor_with_trailing_messages() {
    let context = ContextTracker::default();
    let model = "gpt-4o";
    let provider = "openai";

    let history = vec![
        Message::user("First question"),
        Message::assistant("First answer"),
        Message::user("Follow-up question with some detail"),
    ];

    let anchor = StructuralUsage {
        input_tokens: 5_000,
        output_tokens: 500,
        total_tokens: 5_500,
        ..Default::default()
    };

    // With anchor on assistant turn (idx 1), trigger_tokens reconciles anchor
    // (consumed + output = 5,500) plus trailing user message tokens
    let reconciled = trigger_tokens(&history, Some(&anchor), model, provider, &context);
    let trailing_user_tokens = context.estimate_message_tokens(&history[2], model);
    assert_eq!(reconciled, 5_500 + trailing_user_tokens);

    // Without anchor, returns plain estimate
    let estimated = trigger_tokens(&history, None, model, provider, &context);
    assert!(estimated > 0 && estimated < 100);
}

#[test]
fn test_trigger_tokens_without_assistant_message_uses_anchor_max_fallback() {
    let context = ContextTracker::default();
    let model = "gpt-4o";
    let provider = "openai";

    let history = vec![Message::user("First question without any assistant response yet")];
    let anchor = StructuralUsage {
        input_tokens: 5_000,
        output_tokens: 200,
        total_tokens: 5_200,
        ..Default::default()
    };

    let result = trigger_tokens(&history, Some(&anchor), model, provider, &context);
    let full_estimate = context.calculate_context_tokens(&history, None, model).total_tokens;
    assert_eq!(result, 5_200usize.max(full_estimate));

    // Also verify when anchor tokens is zero, falls back to full estimate
    let zero_anchor = StructuralUsage::default();
    let zero_result = trigger_tokens(&history, Some(&zero_anchor), model, provider, &context);
    assert_eq!(zero_result, full_estimate);
}

#[tokio::test]
async fn test_speculative_compaction_plan_adopted_immediately() {
    let dir = tempfile::tempdir().unwrap();
    let model = MockCompletionModel::default();
    let engine = engine_for(dir.path(), model);
    let presenter = Arc::new(CapturingPresenter::default());

    let speculative_prefix = vec![Message::user("Precomputed speculative summary")];
    let precomputed_plan = super::PatchPlan {
        cut: 2,
        prefix: speculative_prefix.clone(),
    };

    let hook = AutoCompactHook::new(
        engine.session_compactor(),
        presenter,
        engine.usage.clone(),
        engine.context,
        "anthropic",
        50,
    )
    .with_speculative_plan(precomputed_plan);

    let usage = Usage {
        input_tokens: 199_990,
        ..Default::default()
    }
    .into();
    engine
        .usage
        .record_turn(crate::engine::tracking::TurnUsage::new(usage, usage), 100);

    let history = vec![
        Message::user("Turn 1 prompt"),
        Message::assistant("Turn 1 answer"),
        Message::user("Turn 2 prompt"),
    ];

    let action = hook.handle(None, &history, &Message::user("Turn 3 prompt")).await;
    match action {
        CompletionCallAction::Patch(patch) => {
            let patched = patch.history.expect("patch supplies history");
            assert_eq!(patched[0], Message::user("Precomputed speculative summary"));
            assert_eq!(patched[1], Message::user("Turn 2 prompt"));
        }
        other => panic!("expected immediate patch from speculative plan, got {other:?}"),
    }
}

#[tokio::test]
async fn test_speculative_plan_generated_in_lead_band() {
    let dir = tempfile::tempdir().unwrap();
    let model = MockCompletionModel::text("## Goal\nSpeculative goal\n\n## Progress\n- [x] Done");
    let engine = engine_for(dir.path(), model);
    let presenter = Arc::new(CapturingPresenter::default());

    let hook = AutoCompactHook::new(
        engine.session_compactor(),
        presenter,
        engine.usage.clone(),
        engine.context,
        "anthropic",
        10_000,
    );

    let usage = Usage {
        input_tokens: 105_000,
        ..Default::default()
    }
    .into();
    engine
        .usage
        .record_turn(crate::engine::tracking::TurnUsage::new(usage, usage), 100);

    let history = vec![
        Message::user("Turn 1 prompt with details"),
        Message::assistant("Turn 1 response with details"),
        Message::user("Turn 2 prompt with details"),
        Message::assistant("Turn 2 response with details"),
    ];

    let action = hook.handle(None, &history, &Message::user("Turn 3 prompt")).await;
    assert!(matches!(action, CompletionCallAction::Continue));

    let mut generated = false;
    for _ in 0..50 {
        if hook.speculative_plan.lock().unwrap().is_some() {
            generated = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(
        generated,
        "speculative plan should complete asynchronously in background"
    );
}

struct BlockingDemotionHook {
    started: Arc<tokio::sync::Notify>,
    proceed: Arc<tokio::sync::Notify>,
    blocked_once: std::sync::atomic::AtomicBool,
}

impl rig::memory::DemotionHook for BlockingDemotionHook {
    fn on_demote<'a>(
        &'a self,
        _conversation_id: &'a str,
        _messages: Vec<Message>,
    ) -> rig::wasm_compat::WasmBoxedFuture<'a, Result<(), rig::memory::MemoryError>> {
        let started = Arc::clone(&self.started);
        let proceed = Arc::clone(&self.proceed);
        let should_block = self
            .blocked_once
            .compare_exchange(
                false,
                true,
                std::sync::atomic::Ordering::SeqCst,
                std::sync::atomic::Ordering::SeqCst,
            )
            .is_ok();
        Box::pin(async move {
            if should_block {
                started.notify_one();
                proceed.notified().await;
            }
            Ok(())
        })
    }
}

#[tokio::test]
async fn test_speculative_compaction_non_blocking_and_adopted() {
    let dir = tempfile::tempdir().unwrap();
    let model = MockCompletionModel::text("## Goal\nBackground goal\n\n## Progress\n- [x] Done");
    let engine = engine_for(dir.path(), model);
    let presenter = Arc::new(CapturingPresenter::default());

    let started = Arc::new(tokio::sync::Notify::new());
    let proceed = Arc::new(tokio::sync::Notify::new());
    let blocking_hook = Arc::new(BlockingDemotionHook {
        started: Arc::clone(&started),
        proceed: Arc::clone(&proceed),
        blocked_once: std::sync::atomic::AtomicBool::new(false),
    });

    let hook = AutoCompactHook::new(
        engine.session_compactor(),
        presenter,
        engine.usage.clone(),
        engine.context,
        "anthropic",
        10_000,
    )
    .with_demotion_hook(blocking_hook);

    let usage = Usage {
        input_tokens: 105_000,
        ..Default::default()
    }
    .into();
    engine
        .usage
        .record_turn(crate::engine::tracking::TurnUsage::new(usage, usage), 100);

    let history = vec![
        Message::user("Turn 1 prompt with details"),
        Message::assistant("Turn 1 response with details"),
        Message::user("Turn 2 prompt with details"),
        Message::assistant("Turn 2 response with details"),
    ];

    // AC-001: Entering lead band returns immediately without awaiting summarization
    let action = hook.handle(None, &history, &Message::user("Turn 3 prompt")).await;
    assert!(matches!(action, CompletionCallAction::Continue));

    // Wait until background task has reached demotion and is paused
    started.notified().await;
    assert!(hook.in_flight.load(std::sync::atomic::Ordering::SeqCst));
    assert!(hook.speculative_plan.lock().unwrap().is_none());

    // Release background task to finish
    proceed.notify_one();

    // Wait for background task to complete
    let mut completed = false;
    for _ in 0..50 {
        if hook.speculative_plan.lock().unwrap().is_some() {
            completed = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(completed, "speculative plan must be populated on background completion");
    assert!(!hook.in_flight.load(std::sync::atomic::Ordering::SeqCst));

    // AC-002: Hard compaction threshold adopts cached speculative plan immediately
    let hard_usage = Usage {
        input_tokens: 199_990,
        ..Default::default()
    }
    .into();
    engine
        .usage
        .record_turn(crate::engine::tracking::TurnUsage::new(hard_usage, hard_usage), 100);

    let hard_action = hook.handle(None, &history, &Message::user("Turn 4 prompt")).await;
    match hard_action {
        CompletionCallAction::Patch(patch) => {
            let patched = patch.history.expect("patch supplies history");
            assert!(matches!(patched[0], Message::System { .. }));
        }
        other => panic!("expected immediate patch from precomputed speculative plan, got {other:?}"),
    }
}

#[tokio::test]
async fn test_speculative_compaction_fallback_when_task_in_flight() {
    let dir = tempfile::tempdir().unwrap();
    let model = MockCompletionModel::text("## Goal\nFallback goal\n\n## Progress\n- [x] Done");
    let engine = engine_for(dir.path(), model);
    let presenter = Arc::new(CapturingPresenter::default());

    let started = Arc::new(tokio::sync::Notify::new());
    let proceed = Arc::new(tokio::sync::Notify::new());
    let blocking_hook = Arc::new(BlockingDemotionHook {
        started: Arc::clone(&started),
        proceed: Arc::clone(&proceed),
        blocked_once: std::sync::atomic::AtomicBool::new(false),
    });

    let hook = AutoCompactHook::new(
        engine.session_compactor(),
        presenter,
        engine.usage.clone(),
        engine.context,
        "anthropic",
        10_000,
    )
    .with_demotion_hook(blocking_hook);

    let usage = Usage {
        input_tokens: 105_000,
        ..Default::default()
    }
    .into();
    engine
        .usage
        .record_turn(crate::engine::tracking::TurnUsage::new(usage, usage), 100);

    let history = vec![
        Message::user("Turn 1 prompt with details"),
        Message::assistant("Turn 1 response with details"),
        Message::user("Turn 2 prompt with details"),
        Message::assistant("Turn 2 response with details"),
    ];

    let action = hook.handle(None, &history, &Message::user("Turn 3 prompt")).await;
    assert!(matches!(action, CompletionCallAction::Continue));

    started.notified().await;
    assert!(hook.in_flight.load(std::sync::atomic::Ordering::SeqCst));
    assert!(hook.speculative_plan.lock().unwrap().is_none());

    let hard_usage = Usage {
        input_tokens: 199_990,
        ..Default::default()
    }
    .into();
    engine
        .usage
        .record_turn(crate::engine::tracking::TurnUsage::new(hard_usage, hard_usage), 100);

    let hard_action = hook.handle(None, &history, &Message::user("Turn 4 prompt")).await;
    assert!(matches!(hard_action, CompletionCallAction::Patch(_)));

    proceed.notify_one();

    for _ in 0..50 {
        if !hook.in_flight.load(std::sync::atomic::Ordering::SeqCst) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(hook.speculative_plan.lock().unwrap().is_none());
}

#[tokio::test]
async fn test_speculative_plan_discarded_if_history_reset() {
    let dir = tempfile::tempdir().unwrap();
    let model = MockCompletionModel::text("done");
    let engine = engine_for(dir.path(), model);
    let presenter = Arc::new(CapturingPresenter::default());

    let precomputed_plan = super::PatchPlan {
        cut: 10,
        prefix: vec![Message::user("Stale speculative summary")],
    };

    let hook = AutoCompactHook::new(
        engine.session_compactor(),
        presenter,
        engine.usage.clone(),
        engine.context,
        "anthropic",
        50,
    )
    .with_speculative_plan(precomputed_plan);

    let usage = Usage {
        input_tokens: 199_990,
        ..Default::default()
    }
    .into();
    engine
        .usage
        .record_turn(crate::engine::tracking::TurnUsage::new(usage, usage), 100);

    let short_history = vec![Message::user("Reset turn 1")];

    let action = hook.handle(None, &short_history, &Message::user("Reset turn 2")).await;
    match action {
        CompletionCallAction::Continue => {}
        CompletionCallAction::Patch(patch) => {
            let patched = patch.history.expect("patch supplies history");
            assert_ne!(patched[0], Message::user("Stale speculative summary"));
        }
        _ => {}
    }
}

#[tokio::test]
async fn test_speculative_plan_discarded_if_cut_exceeds_base_len() {
    let dir = tempfile::tempdir().unwrap();
    let model = MockCompletionModel::text("done");
    let engine = engine_for(dir.path(), model);
    let presenter = Arc::new(CapturingPresenter::default());

    let invalid_plan = super::PatchPlan {
        cut: 4,
        prefix: vec![Message::user("Summary")],
    };

    let hook = AutoCompactHook::new(
        engine.session_compactor(),
        presenter,
        engine.usage.clone(),
        engine.context,
        "anthropic",
        50,
    )
    .with_speculative_plan(invalid_plan);

    let usage = Usage {
        input_tokens: 199_990,
        ..Default::default()
    }
    .into();
    engine
        .usage
        .record_turn(crate::engine::tracking::TurnUsage::new(usage, usage), 100);

    let history = vec![Message::user("Turn 1"), Message::assistant("Turn 1 response")];

    let action = hook.handle(None, &history, &Message::user("Turn 2 prompt")).await;
    match action {
        CompletionCallAction::Continue => {}
        CompletionCallAction::Patch(patch) => {
            let patched = patch.history.expect("patch supplies history");
            assert_ne!(patched[0], Message::user("Summary"));
        }
        _ => {}
    }
}
