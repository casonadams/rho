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
    let renderer = TerminalRenderer::default();
    let mut batch = super::LiveBatch::new();
    let steering = SharedSteeringQueue::new(crate::engine::runner::QueueMode::All);

    controller.state_mut().editor_mut().set_text("steer this tool");
    let mut ctx = TurnInputContext {
        controller: &mut controller,
        history: &mut history,
        completions: &completions,
        renderer: &renderer,
        batch: &mut batch,
        steering: &steering,
    };

    let result = handle_turn_key(key_event(KeyCode::Enter, KeyModifiers::empty()), &mut ctx).unwrap();
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
    let renderer = TerminalRenderer::default();
    let mut batch = super::LiveBatch::new();
    let steering = SharedSteeringQueue::new(crate::engine::runner::QueueMode::All);

    controller.state_mut().editor_mut().set_text("run after turn");
    let mut ctx = TurnInputContext {
        controller: &mut controller,
        history: &mut history,
        completions: &completions,
        renderer: &renderer,
        batch: &mut batch,
        steering: &steering,
    };

    let result = handle_turn_key(key_event(KeyCode::Enter, KeyModifiers::ALT), &mut ctx).unwrap();
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
    let renderer = TerminalRenderer::default();
    let mut batch = super::LiveBatch::new();
    let steering = SharedSteeringQueue::new(crate::engine::runner::QueueMode::All);

    let mut ctx = TurnInputContext {
        controller: &mut controller,
        history: &mut history,
        completions: &completions,
        renderer: &renderer,
        batch: &mut batch,
        steering: &steering,
    };

    let result = handle_turn_key(key_event(KeyCode::Esc, KeyModifiers::empty()), &mut ctx).unwrap();
    assert!(matches!(result, TurnKeyResult::Cancelled));
}
