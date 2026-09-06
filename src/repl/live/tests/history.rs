use std::fs;

use super::common::HistoryTerminal;
use crate::repl::interactive::InteractiveHistory;
use crate::ui::interactive::{InteractiveState, TerminalController};

fn setup_history_and_controller(path: &std::path::Path) -> (InteractiveHistory, TerminalController<HistoryTerminal>) {
    let mut history = InteractiveHistory::with_file(10, path.to_path_buf()).unwrap();
    history.record("older").unwrap();
    history.record("newer\nsecond").unwrap();
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    controller.state_mut().editor_mut().set_text("draft\nline");
    (history, controller)
}

fn step_previous(controller: &mut TerminalController<HistoryTerminal>, history: &mut InteractiveHistory) -> String {
    assert!(super::super::navigation::navigate_history_previous(controller, history));
    controller.state().editor().text().to_string()
}

fn step_next(controller: &mut TerminalController<HistoryTerminal>, history: &mut InteractiveHistory) -> String {
    assert!(super::super::navigation::navigate_history_next(controller, history));
    controller.state().editor().text().to_string()
}

#[test]
fn active_history_navigation_previous_steps() {
    let path = std::env::temp_dir().join(format!("rho-live-history-{}.txt", uuid::Uuid::new_v4()));
    let (mut history, mut controller) = setup_history_and_controller(&path);

    assert_eq!(step_previous(&mut controller, &mut history), "draft\nline");
    assert_eq!(step_previous(&mut controller, &mut history), "newer\nsecond");
    assert_eq!(step_previous(&mut controller, &mut history), "newer\nsecond");
    assert_eq!(step_previous(&mut controller, &mut history), "older");

    drop(controller);
    drop(history);
    let _ = fs::remove_file(path);
}

#[test]
fn active_history_navigation_next_restores_draft() {
    let path = std::env::temp_dir().join(format!("rho-live-history-{}.txt", uuid::Uuid::new_v4()));
    let (mut history, mut controller) = setup_history_and_controller(&path);

    step_previous(&mut controller, &mut history);
    step_previous(&mut controller, &mut history);
    step_previous(&mut controller, &mut history);
    step_previous(&mut controller, &mut history);

    assert_eq!(step_next(&mut controller, &mut history), "newer\nsecond");
    assert_eq!(step_next(&mut controller, &mut history), "draft\nline");

    drop(controller);
    drop(history);
    let _ = fs::remove_file(path);
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

fn sample_qa_tree() -> rho_harness_core::session::tree::SessionTree {
    use rho_harness_core::session::tree::{SessionTree, TreeNodeData, TreeNodeKind};
    let mut tree = SessionTree::new();
    tree.add_node(TreeNodeData {
        id: "turn-1".into(),
        parent_id: None,
        timestamp: chrono::Utc::now(),
        kind: TreeNodeKind::UserTurn,
        messages: vec![
            rig::message::Message::user("What is life?"),
            rig::message::Message::assistant("42"),
        ],
        label: None,
        metadata: None,
    });
    tree
}

#[test]
fn test_hydrate_session_transcript_populates_items_and_history() {
    let tree = sample_qa_tree();
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    let path = std::env::temp_dir().join(format!("test_hist_{}.txt", uuid::Uuid::new_v4()));
    let mut history = InteractiveHistory::with_file(100, path.clone()).unwrap();

    super::super::navigation::hydrate_session_transcript(&mut controller, &tree, &mut history).unwrap();
    assert_eq!(controller.transcript().len(), 2);
    assert_eq!(history.previous(""), Some("What is life?".to_string()));
    let _ = std::fs::remove_file(path);
}
