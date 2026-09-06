use super::super::InteractiveState;

fn step_up(state: &mut InteractiveState, width: usize) -> (bool, usize) {
    let moved = state.editor_mut().move_up(width);
    (moved, state.editor().cursor())
}

fn step_down(state: &mut InteractiveState, width: usize) -> (bool, usize) {
    let moved = state.editor_mut().move_down(width);
    (moved, state.editor().cursor())
}

#[test]
fn vertical_movement_tracks_the_preferred_column_across_lines() {
    let mut state = InteractiveState::default();
    state.editor_mut().set_text("abcdef\nx\nabcdef");

    assert_eq!(step_up(&mut state, 20), (true, 8));
    assert_eq!(step_up(&mut state, 20), (true, 6));
    assert_eq!(step_up(&mut state, 20), (false, 6));
    assert_eq!(step_down(&mut state, 20), (true, 8));
}

#[test]
fn vertical_movement_uses_visual_wrapped_lines() {
    let mut state = InteractiveState::default();
    state.editor_mut().set_text("abcdefghi");

    assert_eq!(step_up(&mut state, 4), (true, 5));
    assert_eq!(step_up(&mut state, 4), (true, 1));
    assert_eq!(step_up(&mut state, 4), (false, 1));
    assert_eq!(step_down(&mut state, 4), (true, 5));
}

#[test]
fn vertical_movement_preserves_display_column_across_wide_and_short_lines() {
    let mut state = InteractiveState::default();
    state.editor_mut().set_text("a界bc\nx\na界bc");
    let len = state.editor().text().len();

    let up = (step_up(&mut state, 20), step_up(&mut state, 20));
    assert_eq!(up, ((true, 8), (true, 6)));
    let down = (
        step_down(&mut state, 20),
        step_down(&mut state, 20),
        step_down(&mut state, 20),
    );
    assert_eq!(down, ((true, 8), (true, len), (false, len)));
}
