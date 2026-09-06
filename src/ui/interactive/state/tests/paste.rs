use super::super::{InteractiveState, QueueKind, UiAction, UiEffect};

#[test]
fn small_paste_inserts_text_directly() {
    let mut state = InteractiveState::default();
    state.editor_mut().set_text("hello ");
    state.apply(UiAction::Paste("world".to_string()));
    assert_eq!(state.editor().text(), "hello world");
    assert_eq!(state.editor().pastes().len(), 0);
}

#[test]
fn large_multiline_paste_collapses_to_marker() {
    let mut state = InteractiveState::default();
    let lines = (1..=15).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
    state.apply(UiAction::Paste(lines));
    assert_eq!(state.editor().text(), "[paste #1 +15 lines]");
    assert_eq!(state.editor().pastes().len(), 1);

    let UiEffect::Queued(msg) = state.apply(UiAction::Submit(QueueKind::Steering)) else {
        panic!("expected queued message");
    };
    assert!(msg.text.contains("line 1") && msg.text.contains("line 15"));
    assert_eq!((state.editor().text(), state.editor().pastes().len()), ("", 0));
}

#[test]
fn large_single_line_paste_collapses_to_char_marker() {
    let mut state = InteractiveState::default();
    let big_line = "a".repeat(1200);
    state.apply(UiAction::Paste(big_line));
    assert_eq!(state.editor().text(), "[paste #1 1200 chars]");
    assert_eq!(state.editor().pastes().len(), 1);

    let effect = state.apply(UiAction::Submit(QueueKind::Steering));
    let UiEffect::Queued(msg) = effect else {
        panic!("expected queued message");
    };
    assert_eq!(msg.text.len(), 1200);
}

fn state_with_paste(prefix: &str) -> InteractiveState {
    let mut state = InteractiveState::default();
    state.editor_mut().set_text(prefix);
    let lines = (1..=12).map(|i| format!("code {i}")).collect::<Vec<_>>().join("\n");
    state.apply(UiAction::Paste(lines));
    state
}

#[test]
fn atomic_marker_cursor_navigation() {
    let mut state = state_with_paste("prefix ");
    assert_eq!(state.editor().text(), "prefix [paste #1 +12 lines]");
    let marker_end = state.editor().cursor();

    state.apply(UiAction::MoveLeft);
    assert_eq!(state.editor().cursor(), "prefix ".len());

    state.apply(UiAction::MoveRight);
    assert_eq!(state.editor().cursor(), marker_end);
}

#[test]
fn atomic_marker_backspace_deletion() {
    let mut state = state_with_paste("prefix ");
    state.editor_mut().insert_newline();
    state.editor_mut().insert('x');

    state.apply(UiAction::Backspace);
    state.apply(UiAction::Backspace);
    assert_eq!(state.editor().text(), "prefix [paste #1 +12 lines]");

    state.apply(UiAction::Backspace);
    assert_eq!((state.editor().text(), state.editor().pastes().len()), ("prefix ", 0));
}

fn state_with_two_pastes() -> InteractiveState {
    let mut state = InteractiveState::default();
    let p1 = (1..=12).map(|i| format!("first {i}")).collect::<Vec<_>>().join("\n");
    let p2 = (1..=12).map(|i| format!("second {i}")).collect::<Vec<_>>().join("\n");
    state.apply(UiAction::Paste(p1));
    state.editor_mut().insert(' ');
    state.apply(UiAction::Paste(p2));
    state
}

#[test]
fn multi_paste_deletion_renumbers_subsequent_markers() {
    let mut state = state_with_two_pastes();
    assert_eq!(state.editor().text(), "[paste #1 +12 lines] [paste #2 +12 lines]");
    assert_eq!(state.editor().pastes().len(), 2);

    state.apply(UiAction::MoveLeft);
    state.apply(UiAction::MoveLeft);
    state.apply(UiAction::Backspace);
    assert_eq!(
        (state.editor().text(), state.editor().pastes().len()),
        (" [paste #1 +12 lines]", 1)
    );
}

#[test]
fn multi_paste_submit_expansion() {
    let mut state = state_with_two_pastes();
    state.apply(UiAction::MoveLeft);
    state.apply(UiAction::MoveLeft);
    state.apply(UiAction::Backspace);

    let UiEffect::Queued(msg) = state.apply(UiAction::Submit(QueueKind::Steering)) else {
        panic!("expected queued message");
    };
    assert!(msg.text.contains("second 1") && !msg.text.contains("first 1"));
}

#[test]
fn paste_undo_restores_prior_text_and_pastes_map() {
    let mut state = InteractiveState::default();
    state.editor_mut().set_text("before");
    let lines = (1..=15).map(|i| format!("row {i}")).collect::<Vec<_>>().join("\n");
    state.apply(UiAction::Paste(lines));

    assert_eq!(state.editor().text(), "before[paste #1 +15 lines]");
    assert_eq!(state.editor().pastes().len(), 1);

    state.apply(UiAction::Undo);
    assert_eq!(state.editor().text(), "before");
    assert_eq!(state.editor().pastes().len(), 0);
}

#[test]
fn path_paste_prepends_space_after_word_char() {
    let mut state = InteractiveState::default();
    state.editor_mut().set_text("look");
    state.apply(UiAction::Paste("/var/log/syslog".to_string()));
    assert_eq!(state.editor().text(), "look /var/log/syslog");
}
