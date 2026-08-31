use super::{InteractiveState, ModalOption, ModalState, QueueKind, UiAction, UiEffect};

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

#[test]
fn submissions_keep_fifo_order_and_classification() {
    let mut state = InteractiveState::default();
    state.editor_mut().set_text(" steer ");
    assert_eq!(
        state.apply(UiAction::Submit(QueueKind::Steering)),
        UiEffect::Queued(super::QueuedMessage {
            text: "steer".to_string(),
            kind: QueueKind::Steering,
        })
    );
    state.editor_mut().set_text("follow");
    state.apply(UiAction::Submit(QueueKind::FollowUp));

    assert_eq!(state.queue_len(), 2);
    assert_eq!(state.pop_queued().unwrap().kind, QueueKind::Steering);
    assert_eq!(state.pop_queued().unwrap().kind, QueueKind::FollowUp);
}

#[test]
fn dequeue_all_extracts_all_queued_messages() {
    let mut state = InteractiveState::default();
    state.editor_mut().set_text("first");
    state.apply(UiAction::Submit(QueueKind::Steering));
    state.editor_mut().set_text("second");
    state.apply(UiAction::Submit(QueueKind::FollowUp));

    assert_eq!(state.queue_len(), 2);
    let dequeued = state.dequeue_all();
    assert_eq!(dequeued.len(), 2);
    assert_eq!(dequeued[0].text, "first");
    assert_eq!(dequeued[1].text, "second");
    assert_eq!(state.queue_len(), 0);
}

#[test]
fn empty_submissions_are_ignored() {
    let mut state = InteractiveState::default();
    state.editor_mut().set_text(" \n ");
    assert_eq!(state.apply(UiAction::Submit(QueueKind::Steering)), UiEffect::None);
    assert_eq!(state.queue_len(), 0);
}

#[test]
fn tools_expanded_toggle_and_set() {
    let mut state = InteractiveState::default();
    assert!(!state.tools_expanded());

    assert!(state.toggle_tools_expanded());
    assert!(state.tools_expanded());

    assert!(!state.toggle_tools_expanded());
    assert!(!state.tools_expanded());

    state.set_tools_expanded(true);
    assert!(state.tools_expanded());
}

#[test]
fn nested_modals_restore_each_saved_draft_without_changing_queue() {
    let mut state = InteractiveState::default();
    state.editor_mut().set_text("original draft");
    state.apply(UiAction::Submit(QueueKind::Steering));
    state.editor_mut().set_text("next draft");
    state.push_modal(ModalState::new("Approval", "Allow tool?", Vec::<ModalOption>::new()));
    state.editor_mut().set_text("modal response");
    state.push_modal(ModalState::new("Question", "Choose", vec![ModalOption::from("One")]));
    state.editor_mut().set_text("custom answer");

    assert_eq!(state.active_modal().unwrap().title, "Question");
    state.pop_modal();
    assert_eq!(state.editor().text(), "modal response");
    state.pop_modal();
    assert_eq!(state.editor().text(), "next draft");
    assert_eq!(state.queue_len(), 1);
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
