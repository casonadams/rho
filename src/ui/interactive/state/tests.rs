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

    // Expand on submission
    let effect = state.apply(UiAction::Submit(QueueKind::Steering));
    let UiEffect::Queued(msg) = effect else {
        panic!("expected queued message");
    };
    assert!(msg.text.contains("line 1"));
    assert!(msg.text.contains("line 15"));
    assert_eq!(state.editor().text(), "");
    assert_eq!(state.editor().pastes().len(), 0);
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

#[test]
fn atomic_marker_cursor_navigation_and_backspace() {
    let mut state = InteractiveState::default();
    state.editor_mut().set_text("prefix ");
    let lines = (1..=12).map(|i| format!("code {i}")).collect::<Vec<_>>().join("\n");
    state.apply(UiAction::Paste(lines));
    state.editor_mut().insert_newline();
    state.editor_mut().insert('x');

    assert_eq!(state.editor().text(), "prefix [paste #1 +12 lines]\nx");
    assert_eq!(state.editor().cursor(), state.editor().text().len());

    // Backspace 'x' and newline
    state.apply(UiAction::Backspace);
    state.apply(UiAction::Backspace);
    assert_eq!(state.editor().text(), "prefix [paste #1 +12 lines]");
    assert_eq!(state.editor().cursor(), "prefix [paste #1 +12 lines]".len());

    // MoveLeft should leap across the marker to "prefix "
    state.apply(UiAction::MoveLeft);
    assert_eq!(state.editor().cursor(), "prefix ".len());

    // MoveRight should leap across the marker to the end
    state.apply(UiAction::MoveRight);
    assert_eq!(state.editor().cursor(), "prefix [paste #1 +12 lines]".len());

    // Backspace immediately after marker deletes the entire marker
    state.apply(UiAction::Backspace);
    assert_eq!(state.editor().text(), "prefix ");
    assert_eq!(state.editor().cursor(), "prefix ".len());
    assert_eq!(state.editor().pastes().len(), 0);
}

#[test]
fn multi_paste_deletion_renumbers_subsequent_markers() {
    let mut state = InteractiveState::default();
    let p1 = (1..=12).map(|i| format!("first {i}")).collect::<Vec<_>>().join("\n");
    let p2 = (1..=12).map(|i| format!("second {i}")).collect::<Vec<_>>().join("\n");

    state.apply(UiAction::Paste(p1));
    state.editor_mut().insert(' ');
    state.apply(UiAction::Paste(p2));

    assert_eq!(state.editor().text(), "[paste #1 +12 lines] [paste #2 +12 lines]");
    assert_eq!(state.editor().pastes().len(), 2);

    // Move left past paste #2 and space to end of paste #1
    state.apply(UiAction::MoveLeft); // before paste #2
    state.apply(UiAction::MoveLeft); // at end of paste #1: "[paste #1 +12 lines]| [paste #2 +12 lines]"
    assert_eq!(state.editor().cursor(), "[paste #1 +12 lines]".len());

    // Backspace deletes paste #1 and renumbers paste #2 -> paste #1
    state.apply(UiAction::Backspace);
    assert_eq!(state.editor().text(), " [paste #1 +12 lines]");
    assert_eq!(state.editor().pastes().len(), 1);

    // Verify submission expansion has second content
    let effect = state.apply(UiAction::Submit(QueueKind::Steering));
    let UiEffect::Queued(msg) = effect else {
        panic!("expected queued message");
    };
    assert!(msg.text.contains("second 1"));
    assert!(!msg.text.contains("first 1"));
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

#[test]
fn thinking_toggle_state() {
    let mut state = InteractiveState::default();
    assert!(!state.hide_thinking());
    assert!(state.toggle_thinking());
    assert!(state.hide_thinking());
    assert!(!state.toggle_thinking());
    assert!(!state.hide_thinking());
}

#[test]
fn active_tool_lifecycle_and_chunk_accumulation() {
    let mut state = InteractiveState::default();
    assert!(state.active_tool().is_none());

    let mut tool = super::RunningTool::new("bash", "cargo test", None);
    tool.append_chunk("compiling...\n");
    tool.append_chunk("running 5 tests\n");
    assert_eq!(tool.name, "bash");
    assert_eq!(tool.args_summary, "cargo test");
    assert_eq!(tool.output, "compiling...\nrunning 5 tests\n");

    state.set_active_tool(Some(tool));
    assert!(state.active_tool().is_some());
    assert_eq!(state.active_tool().unwrap().output, "compiling...\nrunning 5 tests\n");

    state.active_tool_mut().unwrap().append_chunk("test result: ok\n");
    assert_eq!(
        state.active_tool().unwrap().output,
        "compiling...\nrunning 5 tests\ntest result: ok\n"
    );

    state.set_active_tool(None);
    assert!(state.active_tool().is_none());
}
