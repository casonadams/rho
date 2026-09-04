use std::fs;

use super::common::HistoryTerminal;
use crate::repl::interactive::InteractiveHistory;
use crate::ui::interactive::{InteractiveState, TerminalController};

#[test]
fn active_history_navigation_uses_visual_boundaries_and_restores_the_draft() {
    let path = std::env::temp_dir().join(format!("rho-live-history-{}.txt", uuid::Uuid::new_v4()));
    let mut history = InteractiveHistory::with_file(10, path.clone()).unwrap();
    history.record("older").unwrap();
    history.record("newer\nsecond").unwrap();
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    controller.state_mut().editor_mut().set_text("draft\nline");

    assert!(super::super::navigation::navigate_history_previous(
        &mut controller,
        &mut history
    ));
    assert_eq!(controller.state().editor().text(), "draft\nline");
    assert!(super::super::navigation::navigate_history_previous(
        &mut controller,
        &mut history
    ));
    assert_eq!(controller.state().editor().text(), "newer\nsecond");
    assert!(super::super::navigation::navigate_history_previous(
        &mut controller,
        &mut history
    ));
    assert_eq!(controller.state().editor().text(), "newer\nsecond");
    assert!(super::super::navigation::navigate_history_previous(
        &mut controller,
        &mut history
    ));
    assert_eq!(controller.state().editor().text(), "older");
    assert!(super::super::navigation::navigate_history_next(
        &mut controller,
        &mut history
    ));
    assert_eq!(controller.state().editor().text(), "newer\nsecond");
    assert!(super::super::navigation::navigate_history_next(
        &mut controller,
        &mut history
    ));
    assert_eq!(controller.state().editor().text(), "draft\nline");

    drop(controller);
    drop(history);
    fs::remove_file(path).unwrap();
}

#[test]
fn test_paste_event_collapses_in_interactive_state() {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    let lines = (1..=15).map(|i| format!("code {i}")).collect::<Vec<_>>().join("\n");
    controller
        .state_mut()
        .apply(crate::ui::interactive::UiAction::Paste(lines));
    assert_eq!(controller.state().editor().text(), "[paste #1 +15 lines]");
}

#[test]
fn test_paste_clipboard_callable() {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    let renderer = crate::ui::TerminalRenderer::default();
    super::super::navigation::paste_clipboard(&renderer, &mut controller);
}

#[test]
fn test_hydrate_session_transcript_populates_items_and_history() {
    use rho_harness_core::session::tree::{SessionTree, TreeNodeData, TreeNodeKind};
    let mut tree = SessionTree::new();
    tree.add_node(TreeNodeData {
        id: "turn-1".into(),
        parent_id: None,
        timestamp: chrono::Utc::now(),
        kind: TreeNodeKind::UserTurn,
        messages: vec![
            rig::message::Message::user("What is the meaning of life?"),
            rig::message::Message::assistant("42"),
        ],
        label: None,
        metadata: None,
    });

    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    let history_path = std::env::temp_dir().join(format!("test_hist_{}.txt", uuid::Uuid::new_v4()));
    let mut history = InteractiveHistory::with_file(100, history_path.clone()).unwrap();

    super::super::navigation::hydrate_session_transcript(&mut controller, &tree, &mut history).unwrap();

    assert_eq!(controller.transcript().len(), 2);
    assert!(matches!(
        &controller.transcript()[0],
        crate::ui::interactive::TranscriptItem::UserMessage(text) if text == "What is the meaning of life?"
    ));
    assert!(matches!(
        &controller.transcript()[1],
        crate::ui::interactive::TranscriptItem::AssistantText(text) if text == "42"
    ));

    assert_eq!(history.previous(""), Some("What is the meaning of life?".to_string()));

    let _ = std::fs::remove_file(history_path);
}
