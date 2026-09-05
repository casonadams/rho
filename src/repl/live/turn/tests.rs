use super::input::{TurnInputContext, TurnKeyResult, handle_turn_key, reconcile_consumed_steering};
use crate::repl::coordinator::SharedSteeringQueue;
use crate::repl::interactive::{CompletionSet, InteractiveHistory};
use crate::ui::TerminalRenderer;
use crate::ui::interactive::{InteractiveState, QueueKind, QueuedMessage, TerminalBackend, TerminalController};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use std::io;

struct MockTerminal;
impl TerminalBackend for MockTerminal {
    fn set_raw_mode(&mut self, _: bool) -> io::Result<()> {
        Ok(())
    }
    fn size(&self) -> io::Result<(u16, u16)> {
        Ok((80, 24))
    }
    fn hide_cursor(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn show_cursor(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn move_up(&mut self, _: usize) -> io::Result<()> {
        Ok(())
    }
    fn move_down(&mut self, _: usize) -> io::Result<()> {
        Ok(())
    }
    fn move_to_column(&mut self, _: usize) -> io::Result<()> {
        Ok(())
    }
    fn clear_line(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn write_text(&mut self, _: &str) -> io::Result<()> {
        Ok(())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn key_event(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent {
        code,
        modifiers,
        kind: KeyEventKind::Press,
        state: KeyEventState::empty(),
    }
}

#[tokio::test]
async fn test_turn_input_enter_queues_steering_and_sets_status() {
    let mut controller = TerminalController::new(MockTerminal, InteractiveState::default()).unwrap();
    let history_dir = tempfile::tempdir().unwrap();
    let mut history = InteractiveHistory::with_file(10, history_dir.path().join("history.txt")).unwrap();
    let completions = CompletionSet::from_sources(Default::default());
    let mut batch = super::LiveBatch::new();
    let steering = SharedSteeringQueue::new(crate::engine::runner::QueueMode::All);
    let mut session = crate::repl::ReplSession::new(
        rho_harness_core::config::Config::default(),
        crate::auth::AuthStore::default(),
        None,
    );
    let model_switch = std::sync::Arc::new(rho_engine::engine::runner::SharedModelSwitch::new());

    controller.state_mut().editor_mut().set_text("steer this tool");
    let mut ctx = TurnInputContext {
        controller: &mut controller,
        history: &mut history,
        completions: &completions,
        batch: &mut batch,
        steering: &steering,
        session: &mut session,
        model_switch: &model_switch,
        shared_auth: None,
    };

    let result = handle_turn_key(key_event(KeyCode::Enter, KeyModifiers::empty()), &mut ctx)
        .await
        .unwrap();
    assert!(matches!(result, TurnKeyResult::Handled));

    let polled = crate::engine::runner::SteeringQueueProvider::poll_steering(&steering).await;
    assert_eq!(polled, vec!["steer this tool"]);
    assert_eq!(
        controller.state().system_message(),
        Some("[Steering queued for tool boundary]")
    );

    let queue = controller.state().queue();
    assert_eq!(queue.len(), 1);
    assert_eq!(queue[0].text, "steer this tool");
    assert_eq!(queue[0].kind, QueueKind::Steering);
}

#[tokio::test]
async fn test_turn_input_alt_enter_queues_follow_up_without_steering() {
    let mut controller = TerminalController::new(MockTerminal, InteractiveState::default()).unwrap();
    let history_dir = tempfile::tempdir().unwrap();
    let mut history = InteractiveHistory::with_file(10, history_dir.path().join("history.txt")).unwrap();
    let completions = CompletionSet::from_sources(Default::default());
    let mut batch = super::LiveBatch::new();
    let steering = SharedSteeringQueue::new(crate::engine::runner::QueueMode::All);
    let mut session = crate::repl::ReplSession::new(
        rho_harness_core::config::Config::default(),
        crate::auth::AuthStore::default(),
        None,
    );
    let model_switch = std::sync::Arc::new(rho_engine::engine::runner::SharedModelSwitch::new());

    controller.state_mut().editor_mut().set_text("run after turn");
    let mut ctx = TurnInputContext {
        controller: &mut controller,
        history: &mut history,
        completions: &completions,
        batch: &mut batch,
        steering: &steering,
        session: &mut session,
        model_switch: &model_switch,
        shared_auth: None,
    };

    let result = handle_turn_key(key_event(KeyCode::Enter, KeyModifiers::ALT), &mut ctx)
        .await
        .unwrap();
    assert!(matches!(result, TurnKeyResult::Handled));

    let polled = crate::engine::runner::SteeringQueueProvider::poll_steering(&steering).await;
    assert!(polled.is_empty());
    assert_eq!(
        controller.state().system_message(),
        Some("[Follow-up queued for turn completion]")
    );

    let queue = controller.state().queue();
    assert_eq!(queue.len(), 1);
    assert_eq!(queue[0].text, "run after turn");
    assert_eq!(queue[0].kind, QueueKind::FollowUp);
}

#[tokio::test]
async fn test_reconcile_consumed_steering_removes_consumed_prompts() {
    let mut controller = TerminalController::new(MockTerminal, InteractiveState::default()).unwrap();
    controller.state_mut().push_front_queued(QueuedMessage {
        text: "steer 1".to_string(),
        kind: QueueKind::Steering,
    });
    controller.state_mut().push_front_queued(QueuedMessage {
        text: "follow up".to_string(),
        kind: QueueKind::FollowUp,
    });
    controller.state_mut().push_front_queued(QueuedMessage {
        text: "steer 2".to_string(),
        kind: QueueKind::Steering,
    });

    let steering = SharedSteeringQueue::new(crate::engine::runner::QueueMode::All);
    steering.enqueue("steer 1".to_string());
    let polled = crate::engine::runner::SteeringQueueProvider::poll_steering(&steering).await;
    assert_eq!(polled, vec!["steer 1"]);

    let reconciled = reconcile_consumed_steering(&mut controller, &steering);
    assert!(reconciled);

    let queue: Vec<_> = controller.state().queue().iter().cloned().collect();
    assert_eq!(queue.len(), 2);
    assert_eq!(queue[0].text, "steer 2");
    assert_eq!(queue[1].text, "follow up");
}

#[tokio::test]
async fn test_turn_input_escape_cancels() {
    let mut controller = TerminalController::new(MockTerminal, InteractiveState::default()).unwrap();
    let history_dir = tempfile::tempdir().unwrap();
    let mut history = InteractiveHistory::with_file(10, history_dir.path().join("history.txt")).unwrap();
    let completions = CompletionSet::from_sources(Default::default());
    let mut batch = super::LiveBatch::new();
    let steering = SharedSteeringQueue::new(crate::engine::runner::QueueMode::All);
    let mut session = crate::repl::ReplSession::new(
        rho_harness_core::config::Config::default(),
        crate::auth::AuthStore::default(),
        None,
    );
    let model_switch = std::sync::Arc::new(rho_engine::engine::runner::SharedModelSwitch::new());

    let mut ctx = TurnInputContext {
        controller: &mut controller,
        history: &mut history,
        completions: &completions,
        batch: &mut batch,
        steering: &steering,
        session: &mut session,
        model_switch: &model_switch,
        shared_auth: None,
    };

    let result = handle_turn_key(key_event(KeyCode::Esc, KeyModifiers::empty()), &mut ctx)
        .await
        .unwrap();
    assert!(matches!(result, TurnKeyResult::Cancelled));
}

#[tokio::test]
async fn test_turn_input_ctrl_l_opens_model_selector() {
    let mut controller = TerminalController::new(MockTerminal, InteractiveState::default()).unwrap();
    let history_dir = tempfile::tempdir().unwrap();
    let mut history = InteractiveHistory::with_file(10, history_dir.path().join("history.txt")).unwrap();
    let completions = CompletionSet::from_sources(Default::default());
    let mut batch = super::LiveBatch::new();
    let steering = SharedSteeringQueue::new(crate::engine::runner::QueueMode::All);
    let mut session = crate::repl::ReplSession::new(
        rho_harness_core::config::Config::default(),
        crate::auth::AuthStore::default(),
        None,
    );
    let model_switch = std::sync::Arc::new(rho_engine::engine::runner::SharedModelSwitch::new());

    let mut ctx = TurnInputContext {
        controller: &mut controller,
        history: &mut history,
        completions: &completions,
        batch: &mut batch,
        steering: &steering,
        session: &mut session,
        model_switch: &model_switch,
        shared_auth: None,
    };

    let result = handle_turn_key(key_event(KeyCode::Char('l'), KeyModifiers::CONTROL), &mut ctx)
        .await
        .unwrap();
    assert!(matches!(result, TurnKeyResult::Handled));

    let active_modal = controller.state().active_modal();
    assert!(active_modal.is_some());
    assert_eq!(active_modal.unwrap().title, "Select Model");
}

#[tokio::test]
async fn test_apply_turn_model_switch_updates_model_switch_and_footer() {
    let mut controller = TerminalController::new(MockTerminal, InteractiveState::default()).unwrap();
    let mut batch = super::LiveBatch::new();
    let mut config = rho_harness_core::config::Config::default();
    let auth_store = crate::auth::AuthStore::default();
    let renderer = TerminalRenderer::default();
    let model_switch = std::sync::Arc::new(rho_engine::engine::runner::SharedModelSwitch::new());

    let input = super::TurnModelSwitchInput {
        model: "llama3.2",
        provider: "local",
        save_as_default: false,
        config: &mut config,
        auth_store: &auth_store,
        renderer: &renderer,
        controller: &mut controller,
        model_switch: &model_switch,
        batch: &mut batch,
        shared_auth: None,
    };

    super::apply_turn_model_switch(input).await.unwrap();

    assert_eq!(config.model, "llama3.2");
    assert_eq!(config.provider, "local");
    assert_eq!(model_switch.current_model().as_deref(), Some("llama3.2"));
    assert_eq!(model_switch.current_provider().as_deref(), Some("local"));
    assert!(model_switch.get_handle().is_some());
    assert_eq!(controller.state().footer().model, "llama3.2");
    assert_eq!(
        controller.state().system_message(),
        Some("[Next step will use model: llama3.2 (local)]")
    );
}

#[tokio::test]
async fn test_turn_input_cycle_model_shortcut() {
    let mut controller = TerminalController::new(MockTerminal, InteractiveState::default()).unwrap();
    let history_dir = tempfile::tempdir().unwrap();
    let mut history = InteractiveHistory::with_file(10, history_dir.path().join("history.txt")).unwrap();
    let completions = CompletionSet::from_sources(Default::default());
    let mut batch = super::LiveBatch::new();
    let steering = SharedSteeringQueue::new(crate::engine::runner::QueueMode::All);
    let mut session = crate::repl::ReplSession::new(
        rho_harness_core::config::Config::default(),
        crate::auth::AuthStore::default(),
        None,
    );
    let model_switch = std::sync::Arc::new(rho_engine::engine::runner::SharedModelSwitch::new());

    let mut ctx = TurnInputContext {
        controller: &mut controller,
        history: &mut history,
        completions: &completions,
        batch: &mut batch,
        steering: &steering,
        session: &mut session,
        model_switch: &model_switch,
        shared_auth: None,
    };

    let result = handle_turn_key(key_event(KeyCode::Char('p'), KeyModifiers::CONTROL), &mut ctx)
        .await
        .unwrap();
    assert!(matches!(result, TurnKeyResult::Handled));
}
