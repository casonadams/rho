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
fn word_navigation_and_kill_ring_and_undo_operations() {
    let mut state = InteractiveState::default();
    state.editor_mut().set_text("hello world from test");
    state.apply(UiAction::MoveWordLeft);
    assert_eq!(state.editor().cursor(), 17); // before "test"
    state.apply(UiAction::MoveWordLeft);
    assert_eq!(state.editor().cursor(), 12); // before "from"

    state.apply(UiAction::MoveWordRight);
    assert_eq!(state.editor().cursor(), 16); // after "from"

    // Delete word backward
    state.apply(UiAction::DeleteWordBackward);
    assert_eq!(state.editor().text(), "hello world  test");

    // Yank restored word
    state.apply(UiAction::Yank);
    assert_eq!(state.editor().text(), "hello world from test");

    // Kill to line start
    state.editor_mut().move_to_end();
    state.apply(UiAction::DeleteToLineStart);
    assert_eq!(state.editor().text(), "");

    // Undo
    state.apply(UiAction::Undo);
    assert_eq!(state.editor().text(), "hello world from test");
}
