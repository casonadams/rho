use super::super::{InteractiveState, UiAction};

#[test]
fn editor_inserts_and_deletes_at_unicode_boundaries() {
    let mut state = InteractiveState::default();
    state.editor_mut().set_text("a界c");
    state.apply(UiAction::MoveLeft);
    state.apply(UiAction::Backspace);
    assert_eq!(state.editor().text(), "ac");
    assert_eq!(state.editor().cursor(), 1);

    state.apply(UiAction::Delete);
    assert_eq!(state.editor().text(), "a");
    assert_eq!(state.editor().cursor(), 1);
}

#[test]
fn test_word_navigation() {
    let mut state = InteractiveState::default();
    state.editor_mut().set_text("hello world from test");
    state.apply(UiAction::MoveWordLeft);
    assert_eq!(state.editor().cursor(), 17);
    state.apply(UiAction::MoveWordLeft);
    assert_eq!(state.editor().cursor(), 12);
    state.apply(UiAction::MoveWordRight);
    assert_eq!(state.editor().cursor(), 16);
}

#[test]
fn test_kill_ring_and_undo_operations() {
    let mut state = InteractiveState::default();
    state.editor_mut().set_text("hello world from test");
    state.editor_mut().move_to_start();
    state.apply(UiAction::MoveWordRight);
    state.apply(UiAction::MoveWordRight);
    state.apply(UiAction::MoveWordRight);

    state.apply(UiAction::DeleteWordBackward);
    assert_eq!(state.editor().text(), "hello world  test");

    state.apply(UiAction::Yank);
    assert_eq!(state.editor().text(), "hello world from test");

    state.editor_mut().move_to_end();
    state.apply(UiAction::DeleteToLineStart);
    assert_eq!(state.editor().text(), "");

    state.apply(UiAction::Undo);
    assert_eq!(state.editor().text(), "hello world from test");
}
