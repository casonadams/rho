use crate::repl::interactive::InteractiveHistory;
use crate::ui::interactive::NullBackend;
use crate::ui::interactive::{InteractiveState, TerminalController};

fn setup_history_and_controller(path: &std::path::Path) -> (InteractiveHistory, TerminalController<NullBackend>) {
    let mut history = InteractiveHistory::with_file(10, path.to_path_buf()).unwrap();
    history.record("older").unwrap();
    history.record("newer\nsecond").unwrap();
    let mut controller = TerminalController::new(NullBackend, InteractiveState::default()).unwrap();
    controller.state_mut().editor_mut().set_text("draft\nline");
    (history, controller)
}

fn step_previous(controller: &mut TerminalController<NullBackend>, history: &mut InteractiveHistory) -> String {
    assert!(super::super::navigation::navigate_history_previous(controller, history));
    controller.state().editor().text().to_string()
}

fn step_next(controller: &mut TerminalController<NullBackend>, history: &mut InteractiveHistory) -> String {
    assert!(super::super::navigation::navigate_history_next(controller, history));
    controller.state().editor().text().to_string()
}

#[test]
fn active_history_navigation_previous_steps() {
    let dir = rho_test_support::TestDir::new();
    let path = dir.join("history.txt");
    let (mut history, mut controller) = setup_history_and_controller(&path);

    assert_eq!(step_previous(&mut controller, &mut history), "draft\nline");
    assert_eq!(step_previous(&mut controller, &mut history), "newer\nsecond");
    assert_eq!(step_previous(&mut controller, &mut history), "newer\nsecond");
    assert_eq!(step_previous(&mut controller, &mut history), "older");
}

#[test]
fn active_history_navigation_next_restores_draft() {
    let dir = rho_test_support::TestDir::new();
    let path = dir.join("history.txt");
    let (mut history, mut controller) = setup_history_and_controller(&path);

    step_previous(&mut controller, &mut history);
    step_previous(&mut controller, &mut history);
    step_previous(&mut controller, &mut history);
    step_previous(&mut controller, &mut history);

    assert_eq!(step_next(&mut controller, &mut history), "newer\nsecond");
    assert_eq!(step_next(&mut controller, &mut history), "draft\nline");
}

#[test]
fn test_paste_event_collapses_in_interactive_state() {
    let mut controller = TerminalController::new(NullBackend, InteractiveState::default()).unwrap();
    let lines = (1..=15).map(|i| format!("code {i}")).collect::<Vec<_>>().join("\n");
    controller
        .state_mut()
        .apply(crate::ui::interactive::UiAction::Paste(lines));
    assert_eq!(controller.state().editor().text(), "[paste #1 +15 lines]");
}

#[tokio::test]
async fn test_paste_clipboard_async_callable() {
    let mut controller = TerminalController::new(NullBackend, InteractiveState::default()).unwrap();
    let renderer = crate::ui::TerminalRenderer::default();
    super::super::navigation::paste_clipboard_async(&renderer, &mut controller).await;
}

#[test]
fn test_ghost_text_acceptance_on_move_right() {
    let mut editor = crate::ui::interactive::EditorState::default();
    editor.set_text("git stat");
    editor.set_ghost_text(Some("us".to_string()));

    assert_eq!(editor.ghost_text(), Some("us"));
    assert!(editor.accept_ghost_text());
    assert_eq!(editor.text(), "git status");
    assert_eq!(editor.cursor(), 10);
    assert_eq!(editor.ghost_text(), None);
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
            rho_harness_core::model::ChatMessage::user("What is life?"),
            rho_harness_core::model::ChatMessage::assistant("42"),
        ],
        label: None,
        metadata: None,
    });
    tree
}

#[test]
fn test_hydrate_session_transcript_populates_items_and_history() {
    let tree = sample_qa_tree();
    let mut controller = TerminalController::new(NullBackend, InteractiveState::default()).unwrap();
    let dir = rho_test_support::TestDir::new();
    let mut history = InteractiveHistory::with_file(100, dir.join("history.txt")).unwrap();

    super::super::navigation::hydrate_session_transcript(&mut controller, &tree, &mut history).unwrap();
    assert_eq!(controller.transcript().len(), 2);
    assert_eq!(history.previous(""), Some("What is life?".to_string()));
}
