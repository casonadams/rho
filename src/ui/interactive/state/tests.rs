use super::*;
use crate::ui::interactive::OptionLayout;

// --- Editor tests ---

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

// --- Modal tests ---

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
fn modal_filter_fuzzy_matches_subsequences_ranked() {
    let mut modal = ModalState::new(
        "Select Model",
        "",
        vec![
            ModalOption::new("gemini-2.5-flash", Some("[antigravity]")),
            ModalOption::new("gemini-3.8-flash", Some("[antigravity]")),
            ModalOption::new("gemini-3.1-pro", Some("[antigravity]")),
            ModalOption::new("claude-sonnet-4-6", Some("[antigravity]")),
        ],
    )
    .with_search(true);

    modal.set_filter("gemin");
    assert_eq!(modal.options.len(), 3);

    modal.set_filter("gem3");
    let ids: Vec<&str> = modal.options.iter().map(|o| o.label.as_str()).collect();
    assert_eq!(ids, vec!["gemini-3.8-flash", "gemini-3.1-pro"]);

    modal.set_filter("claude");
    assert_eq!(modal.options[0].label, "claude-sonnet-4-6");

    modal.set_filter("");
    assert_eq!(modal.options.len(), 4);
}

#[test]
fn test_modal_state_option_layout() {
    let modal = ModalState::new("Test", "body", vec![]);
    assert_eq!(modal.option_layout, OptionLayout::Vertical);
    let horizontal_modal = modal.with_option_layout(OptionLayout::Horizontal);
    assert_eq!(horizontal_modal.option_layout, OptionLayout::Horizontal);
}

#[test]
fn test_modal_state_scroll_down_and_up() {
    let mut modal = ModalState::new("Test", "body", vec![]);
    assert_eq!(modal.body_scroll, 0);

    modal.scroll_body_down(5);
    modal.scroll_body_down(5);
    assert_eq!(modal.body_scroll, 2);

    modal.scroll_body_up();
    assert_eq!(modal.body_scroll, 1);
    modal.scroll_body_up();
    modal.scroll_body_up();
    assert_eq!(modal.body_scroll, 0);
}

#[test]
fn test_modal_state_clamp_body_scroll() {
    let mut modal = ModalState::new("Test", "body", vec![]);
    modal.body_scroll = 10;
    modal.clamp_body_scroll(4);
    assert_eq!(modal.body_scroll, 4);
}

// --- Navigation tests ---

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

// --- Paste tests ---

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

// --- Queue tests ---

#[test]
fn submissions_keep_fifo_order_and_classification() {
    let mut state = InteractiveState::default();
    state.editor_mut().set_text(" steer ");
    assert_eq!(
        state.apply(UiAction::Submit(QueueKind::Steering)),
        UiEffect::Queued(QueuedMessage {
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
    assert_eq!(
        (
            dequeued.len(),
            dequeued[0].text.as_str(),
            dequeued[1].text.as_str(),
            state.queue_len()
        ),
        (2, "first", "second", 0)
    );
}

#[test]
fn empty_submissions_are_ignored() {
    let mut state = InteractiveState::default();
    state.editor_mut().set_text(" \n ");
    assert_eq!(state.apply(UiAction::Submit(QueueKind::Steering)), UiEffect::None);
    assert_eq!(state.queue_len(), 0);
}

// --- Running tool tests ---

#[test]
fn tools_expanded_toggle() {
    let mut state = InteractiveState::default();
    assert_eq!((state.toggle_tools_expanded(), state.tools_expanded()), (true, true));
    assert_eq!((state.toggle_tools_expanded(), state.tools_expanded()), (false, false));
}

#[test]
fn tools_expanded_set() {
    let mut state = InteractiveState::default();
    state.set_tools_expanded(true);
    assert!(state.tools_expanded());
}

#[test]
fn thinking_toggle_state() {
    let mut state = InteractiveState::default();
    assert!(!state.hide_thinking());
    assert_eq!((state.toggle_thinking(), state.hide_thinking()), (true, true));
    assert_eq!((state.toggle_thinking(), state.hide_thinking()), (false, false));
}

#[test]
fn active_tool_chunk_accumulation() {
    let mut tool = RunningTool::new("bash", "cargo test", None);
    tool.append_chunk("compiling...\n");
    tool.append_chunk("running 5 tests\n");
    assert_eq!((tool.name.as_str(), tool.args_summary.as_str()), ("bash", "cargo test"));
    assert_eq!(tool.output, "compiling...\nrunning 5 tests\n");
}

#[test]
fn active_tool_lifecycle() {
    let mut state = InteractiveState::default();
    assert!(state.active_tool().is_none());

    let mut tool = RunningTool::new("bash", "cargo test", None);
    tool.append_chunk("compiling...\n");
    state.set_active_tool(Some(tool));
    assert_eq!(state.active_tool().unwrap().output, "compiling...\n");

    state.active_tool_mut().unwrap().append_chunk("ok\n");
    assert_eq!(state.active_tool().unwrap().output, "compiling...\nok\n");

    state.set_active_tool(None);
    assert!(state.active_tool().is_none());
}

fn assert_tail_truncation(output: &str) {
    let max = MAX_RUNNING_BUFFER_BYTES;
    assert!(output.len() <= max);
    assert!(output.ends_with("line 5000: detailed execution log output\n"));
    assert!(!output.contains("line 0001:") && output.starts_with("line "));
}

#[test]
fn running_tool_rolling_tail_truncation_under_massive_chunks() {
    let mut tool = RunningTool::new("bash", "seq 1 10000", None);
    for i in 1..=5000 {
        tool.append_chunk(&format!("line {i:04}: detailed execution log output\n"));
    }
    assert_tail_truncation(&tool.output);
}

#[test]
fn running_tool_preserves_utf8_char_and_newline_boundaries() {
    let mut tool = RunningTool::new("bash", "unicode stream", None);
    let line = "🦀 🚀 🔥 ✨ 这是一个很长的多字节测试行 🌟 💫\n";
    for _ in 0..2500 {
        tool.append_chunk(line);
    }

    assert!(tool.output.len() <= MAX_RUNNING_BUFFER_BYTES);
    assert!(
        tool.output.starts_with("🦀 "),
        "trimmed output must start at the beginning of a line on a valid character boundary"
    );
    assert!(tool.output.ends_with("🌟 💫\n"));
}

#[test]
fn running_tool_single_massive_chunk_bounded() {
    let mut tool = RunningTool::new("bash", "massive chunk", None);
    let massive_chunk = "alpha beta gamma delta epsilon\n".repeat(8000);
    tool.append_chunk(&massive_chunk);

    assert!(tool.output.len() <= MAX_RUNNING_BUFFER_BYTES);
    assert!(tool.output.starts_with("alpha "));
    assert!(tool.output.ends_with("epsilon\n"));
}

#[test]
fn running_tool_massive_line_without_newlines_bounded_safely() {
    let mut tool = RunningTool::new("bash", "one line", None);
    let massive_line = "🔥abc🚀def".repeat(15000);
    tool.append_chunk(&massive_line);

    assert!(tool.output.len() <= MAX_RUNNING_OUTPUT_BYTES);
    assert!(tool.output.is_char_boundary(0));
}

#[test]
fn running_tool_preserves_ansi_escapes_across_chunk_boundaries() {
    let mut tool = RunningTool::new("bash", "test", None);
    tool.append_chunk("compiling \x1b[4");
    tool.append_chunk("0mwith fill\x1b[0m done\n");
    assert_eq!(tool.output, "compiling \x1b[40mwith fill\x1b[0m done\n");
}

#[test]
fn activity_label_maps_variants() {
    assert_eq!(Activity::Idle.label(), "idle");
    assert_eq!(Activity::Thinking.label(), "thinking");
    assert_eq!(Activity::Compacting.label(), "compacting");
    assert_eq!(Activity::Working.label(), "working");
}

#[test]
fn footer_state_equality_and_inequality() {
    let base = FooterState {
        activity: Activity::Idle,
        running_tool: Some("bash".into()),
        provider: "anthropic".into(),
        model: "claude".into(),
        thinking_level: Some("high".into()),
        cwd: Some("/tmp".into()),
        git_branch: Some("main".into()),
        session_name: Some("test".into()),
        quota: Some("100%".into()),
        context_percent: Some(0.5),
        context_window: 200_000,
        total_input_tokens: 1000,
        total_output_tokens: 200,
        total_cache_read_tokens: 50,
        total_cache_write_tokens: 25,
        total_cost: Some(0.015),
        tokens_per_second: Some(45.2),
        extra_status: Some("ready".into()),
        hidden_status_count: 2,
        context: Some("ctx".into()),
        show_label: true,
        remote_active: true,
        remote_peers: 3,
    };
    assert_eq!(base, base.clone());

    let mut diff_id = base.clone();
    diff_id.model = "gpt-4o".into();
    assert_ne!(base, diff_id);

    let mut diff_metrics = base.clone();
    diff_metrics.total_cost = Some(0.02);
    assert_ne!(base, diff_metrics);

    let mut diff_ui = base.clone();
    diff_ui.remote_peers = 4;
    assert_ne!(base, diff_ui);
}
