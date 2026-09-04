use super::super::InteractiveState;

#[test]
fn vertical_movement_tracks_the_preferred_column_across_lines() {
    let mut state = InteractiveState::default();
    state.editor_mut().set_text("abcdef\nx\nabcdef");

    assert!(state.editor_mut().move_up(20));
    assert_eq!(state.editor().cursor(), 8);
    assert!(state.editor_mut().move_up(20));
    assert_eq!(state.editor().cursor(), 6);
    assert!(!state.editor_mut().move_up(20));
    assert!(state.editor_mut().move_down(20));
    assert_eq!(state.editor().cursor(), 8);
}

#[test]
fn vertical_movement_uses_visual_wrapped_lines() {
    let mut state = InteractiveState::default();
    state.editor_mut().set_text("abcdefghi");

    assert!(state.editor_mut().move_up(4));
    assert_eq!(state.editor().cursor(), 5);
    assert!(state.editor_mut().move_up(4));
    assert_eq!(state.editor().cursor(), 1);
    assert!(!state.editor_mut().move_up(4));
    assert!(state.editor_mut().move_down(4));
    assert_eq!(state.editor().cursor(), 5);
}

#[test]
fn vertical_movement_preserves_display_column_across_wide_and_short_lines() {
    let mut state = InteractiveState::default();
    state.editor_mut().set_text("a界bc\nx\na界bc");

    assert!(state.editor_mut().move_up(20));
    assert_eq!(state.editor().cursor(), 8);
    assert!(state.editor_mut().move_up(20));
    assert_eq!(state.editor().cursor(), 6);
    assert!(state.editor_mut().move_down(20));
    assert_eq!(state.editor().cursor(), 8);
    assert!(state.editor_mut().move_down(20));
    assert_eq!(state.editor().cursor(), state.editor().text().len());
    assert!(!state.editor_mut().move_down(20));
}
