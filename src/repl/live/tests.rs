use super::live_ui_supported;
use super::navigation::{navigate_history_next, navigate_history_previous};
use crate::repl::interactive::InteractiveHistory;
use crate::ui::interactive::{InteractiveState, TerminalBackend, TerminalController};
use std::{fs, io};

struct HistoryTerminal;

impl TerminalBackend for HistoryTerminal {
    fn set_raw_mode(&mut self, _enabled: bool) -> io::Result<()> {
        Ok(())
    }

    fn size(&self) -> io::Result<(u16, u16)> {
        Ok((20, 24))
    }

    fn hide_cursor(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn show_cursor(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn move_up(&mut self, _rows: usize) -> io::Result<()> {
        Ok(())
    }

    fn move_down(&mut self, _rows: usize) -> io::Result<()> {
        Ok(())
    }

    fn move_to_column(&mut self, _column: usize) -> io::Result<()> {
        Ok(())
    }

    fn clear_line(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn write_text(&mut self, _text: &str) -> io::Result<()> {
        Ok(())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn live_ui_requires_both_terminal_streams() {
    assert!(live_ui_supported(true, true));
    assert!(!live_ui_supported(true, false));
    assert!(!live_ui_supported(false, true));
    assert!(!live_ui_supported(false, false));
}

#[test]
fn active_history_navigation_uses_visual_boundaries_and_restores_the_draft() {
    let path = std::env::temp_dir().join(format!("rho-live-history-{}.txt", uuid::Uuid::new_v4()));
    let mut history = InteractiveHistory::with_file(10, path.clone()).unwrap();
    history.record("older").unwrap();
    history.record("newer\nsecond").unwrap();
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    controller.state_mut().editor_mut().set_text("draft\nline");

    assert!(navigate_history_previous(&mut controller, &mut history));
    assert_eq!(controller.state().editor().text(), "draft\nline");
    assert!(navigate_history_previous(&mut controller, &mut history));
    assert_eq!(controller.state().editor().text(), "newer\nsecond");
    assert!(navigate_history_previous(&mut controller, &mut history));
    assert_eq!(controller.state().editor().text(), "newer\nsecond");
    assert!(navigate_history_previous(&mut controller, &mut history));
    assert_eq!(controller.state().editor().text(), "older");
    assert!(navigate_history_next(&mut controller, &mut history));
    assert_eq!(controller.state().editor().text(), "newer\nsecond");
    assert!(navigate_history_next(&mut controller, &mut history));
    assert_eq!(controller.state().editor().text(), "draft\nline");

    drop(controller);
    drop(history);
    fs::remove_file(path).unwrap();
}

#[test]
fn live_batch_flushes_tool_end_with_transcript_without_intermediate_redraw() {
    let mut batch = super::batch::LiveBatch::new();
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    controller
        .start_tool(crate::ui::interactive::ToolStartRequest {
            name: "bash".into(),
            args_summary: "cargo test".into(),
            preview: None,
        })
        .unwrap();

    batch
        .enqueue(&mut controller, crate::ui::interactive::UiEvent::ToolEnd)
        .unwrap();
    batch
        .enqueue(
            &mut controller,
            crate::ui::interactive::UiEvent::Transcript(crate::ui::interactive::TranscriptItem::Tool(
                crate::ui::interactive::ToolItem {
                    name: "bash".into(),
                    arguments: serde_json::json!({"command": "cargo test"}),
                    is_error: false,
                    output: "all tests passed".into(),
                    output_summary: "ok".into(),
                    duration_ms: Some(50),
                },
            )),
        )
        .unwrap();

    batch.flush(&mut controller, false).unwrap();
    assert_eq!(controller.transcript().len(), 1);
}

#[test]
fn model_selector_modal_filtering_and_selection() {
    let config = rho_harness_core::config::Config::default();
    let auth_store = crate::auth::AuthStore::load(&config.auth_file).unwrap_or_default();
    let session = crate::repl::ReplSession::new(config, auth_store, None);
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();

    // Open model selector
    super::modal::open_model_selector(&session, &mut controller);
    assert_eq!(controller.state().active_modal().unwrap().title, "Select Model");

    // Filter by "sonnet"
    let key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('s'),
        crossterm::event::KeyModifiers::NONE,
    );
    let res = super::modal::handle_modal_key(&mut controller, key, &mut None).unwrap();
    assert_eq!(res, super::modal::ModalKeyResult::Handled);
    assert_eq!(controller.state().active_modal().unwrap().filter_query, "s");

    // Filter with "claude"
    if let Some(modal) = controller.state_mut().active_modal_mut() {
        modal.set_filter("claude");
    }
    let modal = controller.state().active_modal().unwrap();
    assert!(modal.options.iter().any(|o| o.label.contains("claude")));

    // Select with Enter
    let enter_key =
        crossterm::event::KeyEvent::new(crossterm::event::KeyCode::Enter, crossterm::event::KeyModifiers::NONE);
    let res = super::modal::handle_modal_key(&mut controller, enter_key, &mut None).unwrap();
    match res {
        super::modal::ModalKeyResult::ModelSelected {
            model,
            provider,
            save_as_default,
        } => {
            assert!(model.contains("claude"));
            assert!(!provider.is_empty());
            assert!(!save_as_default);
        }
        _ => panic!("expected ModelSelected result"),
    }
    assert!(controller.state().active_modal().is_none());
}

#[test]
fn tree_selector_modal_selection() {
    let mut tree = rho_harness_core::session::tree::SessionTree::new();
    tree.add_node(rho_harness_core::session::tree::TreeNodeData {
        id: "node-1".into(),
        parent_id: None,
        timestamp: chrono::Utc::now(),
        kind: rho_harness_core::session::tree::TreeNodeKind::UserTurn,
        messages: vec![rig::message::Message::user("Hello")],
        label: Some("checkpoint-1".into()),
        metadata: None,
    });
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();

    super::modal::open_tree_selector(&tree, &mut controller);
    assert_eq!(controller.state().active_modal().unwrap().title, "Conversation Tree");

    let enter_key =
        crossterm::event::KeyEvent::new(crossterm::event::KeyCode::Enter, crossterm::event::KeyModifiers::NONE);
    let res = super::modal::handle_modal_key(&mut controller, enter_key, &mut None).unwrap();
    assert_eq!(
        res,
        super::modal::ModalKeyResult::TreeNodeSelected {
            node_id: "node-1".into()
        }
    );
    assert!(controller.state().active_modal().is_none());
}

#[test]
fn tree_selector_modal_shift_l_labels_checkpoint() {
    let mut tree = rho_harness_core::session::tree::SessionTree::new();
    tree.add_node(rho_harness_core::session::tree::TreeNodeData {
        id: "node-42".into(),
        parent_id: None,
        timestamp: chrono::Utc::now(),
        kind: rho_harness_core::session::tree::TreeNodeKind::UserTurn,
        messages: vec![rig::message::Message::user("Hello")],
        label: None,
        metadata: None,
    });
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    super::modal::open_tree_selector(&tree, &mut controller);

    let shift_l = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('L'),
        crossterm::event::KeyModifiers::SHIFT,
    );
    let res = super::modal::handle_modal_key(&mut controller, shift_l, &mut None).unwrap();
    assert_eq!(res, super::modal::ModalKeyResult::Handled);
    assert!(matches!(
        controller.state().active_modal().unwrap().mode,
        crate::ui::interactive::ModalMode::Input { .. }
    ));

    let char_a = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('a'),
        crossterm::event::KeyModifiers::NONE,
    );
    let _ = super::modal::handle_modal_key(&mut controller, char_a, &mut None).unwrap();
    let char_b = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('b'),
        crossterm::event::KeyModifiers::NONE,
    );
    let _ = super::modal::handle_modal_key(&mut controller, char_b, &mut None).unwrap();

    let enter_key =
        crossterm::event::KeyEvent::new(crossterm::event::KeyCode::Enter, crossterm::event::KeyModifiers::NONE);
    let res = super::modal::handle_modal_key(&mut controller, enter_key, &mut None).unwrap();
    assert_eq!(
        res,
        super::modal::ModalKeyResult::NodeLabelUpdated {
            node_id: "node-42".into(),
            label: "ab".into(),
        }
    );
    assert!(controller.state().active_modal().is_none());
}

#[test]
fn session_selector_modal_selection() {
    let temp_dir = std::env::temp_dir().join(format!("test_sessions_{}", uuid::Uuid::new_v4()));
    let manager = rho_harness_core::session::SessionManager::new(&temp_dir, None).unwrap();
    let session_id = manager.session_id.clone();

    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    super::modal::open_session_selector(&temp_dir, &mut controller);
    assert_eq!(controller.state().active_modal().unwrap().title, "Resume Session");

    let enter_key =
        crossterm::event::KeyEvent::new(crossterm::event::KeyCode::Enter, crossterm::event::KeyModifiers::NONE);
    let res = super::modal::handle_modal_key(&mut controller, enter_key, &mut None).unwrap();
    assert_eq!(res, super::modal::ModalKeyResult::SessionSelected { session_id });
    assert!(controller.state().active_modal().is_none());
    let _ = std::fs::remove_dir_all(temp_dir);
}

#[test]
fn session_selector_modal_ctrl_d_deletes_session() {
    let temp_dir = std::env::temp_dir().join(format!("test_sessions_del_{}", uuid::Uuid::new_v4()));
    let manager = rho_harness_core::session::SessionManager::new(&temp_dir, None).unwrap();
    let session_id = manager.session_id.clone();

    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    super::modal::open_session_selector(&temp_dir, &mut controller);

    let ctrl_d = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('d'),
        crossterm::event::KeyModifiers::CONTROL,
    );
    let res = super::modal::handle_modal_key(&mut controller, ctrl_d, &mut None).unwrap();
    assert_eq!(
        res,
        super::modal::ModalKeyResult::SessionDeleted {
            session_id: session_id.clone()
        }
    );
    assert!(controller.state().active_modal().unwrap().options.is_empty());
    let _ = std::fs::remove_dir_all(temp_dir);
}

#[test]
fn settings_selector_modal_toggles() {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    assert!(!controller.state().hide_thinking());
    assert!(!controller.state().tools_expanded());

    super::modal::open_settings_selector(&mut controller);
    assert_eq!(controller.state().active_modal().unwrap().title, "Settings");

    let enter_key =
        crossterm::event::KeyEvent::new(crossterm::event::KeyCode::Enter, crossterm::event::KeyModifiers::NONE);
    let res = super::modal::handle_modal_key(&mut controller, enter_key, &mut None).unwrap();
    assert_eq!(res, super::modal::ModalKeyResult::Handled);
    assert!(controller.state().hide_thinking());
    assert!(
        controller.state().active_modal().unwrap().options[0]
            .description
            .as_ref()
            .unwrap()
            .contains("Hidden")
    );

    let down_key =
        crossterm::event::KeyEvent::new(crossterm::event::KeyCode::Down, crossterm::event::KeyModifiers::NONE);
    let _ = super::modal::handle_modal_key(&mut controller, down_key, &mut None).unwrap();
    let res = super::modal::handle_modal_key(&mut controller, enter_key, &mut None).unwrap();
    assert_eq!(res, super::modal::ModalKeyResult::Handled);
    assert!(controller.state().tools_expanded());
    assert!(
        controller.state().active_modal().unwrap().options[1]
            .description
            .as_ref()
            .unwrap()
            .contains("Expanded")
    );
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
    super::navigation::paste_clipboard(&renderer, &mut controller);
}

#[test]
fn test_hydrate_session_transcript_populates_items_and_history() {
    use rho_harness_core::session::tree::{SessionTree, TreeNodeData, TreeNodeKind};
    let mut tree = SessionTree::new();
    tree.add_node(TreeNodeData {
        id: "turn-1".into(),
        parent_id: None,
        timestamp: chrono::Utc::now(),
        kind: TreeNodeKind::UserTurn,
        messages: vec![
            rig::message::Message::user("What is the meaning of life?"),
            rig::message::Message::assistant("42"),
        ],
        label: None,
        metadata: None,
    });

    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    let history_path = std::env::temp_dir().join(format!("test_hist_{}.txt", uuid::Uuid::new_v4()));
    let mut history = InteractiveHistory::with_file(100, history_path.clone()).unwrap();

    super::navigation::hydrate_session_transcript(&mut controller, &tree, &mut history).unwrap();

    assert_eq!(controller.transcript().len(), 2);
    assert!(matches!(
        &controller.transcript()[0],
        crate::ui::interactive::TranscriptItem::UserMessage(text) if text == "What is the meaning of life?"
    ));
    assert!(matches!(
        &controller.transcript()[1],
        crate::ui::interactive::TranscriptItem::AssistantText(text) if text == "42"
    ));

    assert_eq!(history.previous(""), Some("What is the meaning of life?".to_string()));

    let _ = std::fs::remove_file(history_path);
}

#[tokio::test]
async fn test_user_bash_runner_streams_and_completes() {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    let (_events_tx, mut events_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut input_reader = crate::repl::input_reader::TerminalInputReader::spawn_dummy();

    let renderer = crate::ui::TerminalRenderer::default();
    let mut live_io = super::LiveIo {
        controller: &mut controller,
        events: &mut events_rx,
        input: &mut input_reader,
    };

    let res = super::bash_runner::run_user_bash("echo 'hello from user bash'", &renderer, &mut live_io)
        .await
        .unwrap();

    assert!(!res.is_cancelled);
    assert!(!res.is_error);
    assert!(res.output.contains("hello from user bash"));
}

#[tokio::test]
async fn test_user_bash_runner_cancellation_preempts_and_terminates() {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    let (_events_tx, mut events_rx) = tokio::sync::mpsc::unbounded_channel();
    let cancel_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Esc,
        crossterm::event::KeyModifiers::empty(),
    ));
    let mut input_reader = crate::repl::input_reader::TerminalInputReader::spawn_with_events(vec![cancel_event]);

    let renderer = crate::ui::TerminalRenderer::default();
    let mut live_io = super::LiveIo {
        controller: &mut controller,
        events: &mut events_rx,
        input: &mut input_reader,
    };

    let res = super::bash_runner::run_user_bash("sleep 30 & wait", &renderer, &mut live_io)
        .await
        .unwrap();

    assert!(res.is_cancelled);
    assert!(res.is_error);
}

#[tokio::test]
async fn test_user_bash_runner_large_output_spools_to_disk() {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    let (_events_tx, mut events_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut input_reader = crate::repl::input_reader::TerminalInputReader::spawn_dummy();

    let renderer = crate::ui::TerminalRenderer::default();
    let mut live_io = super::LiveIo {
        controller: &mut controller,
        events: &mut events_rx,
        input: &mut input_reader,
    };

    let res = super::bash_runner::run_user_bash("seq 1 2500", &renderer, &mut live_io)
        .await
        .unwrap();

    assert!(!res.is_cancelled);
    assert!(!res.is_error);
    assert!(res.output.contains("[Showing lines "));
    assert!(res.output.contains("of 2500"));
    assert!(res.output.contains("Full output: "));
    assert!(res.output.contains("rho-bash-"));

    let start_marker = "Full output: ";
    let start_idx = res
        .output
        .find(start_marker)
        .expect("spool marker must be present in output");
    let after = &res.output[start_idx + start_marker.len()..];
    let end_idx = after.find(']').expect("closing bracket must terminate path");
    let path_str = &after[..end_idx];
    let path = std::path::Path::new(path_str);
    assert!(path.exists(), "temp spool log should exist at {path_str}");

    let spooled = std::fs::read_to_string(path).expect("spool log should be readable");
    assert!(spooled.starts_with("1\n"));
    assert!(spooled.ends_with("2500\n"));
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn test_user_bash_runner_failed_command_includes_exit_code() {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    let (_events_tx, mut events_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut input_reader = crate::repl::input_reader::TerminalInputReader::spawn_dummy();

    let renderer = crate::ui::TerminalRenderer::default();
    let mut live_io = super::LiveIo {
        controller: &mut controller,
        events: &mut events_rx,
        input: &mut input_reader,
    };

    let res =
        super::bash_runner::run_user_bash("sh -c 'echo \"failure details\" >&2; exit 42'", &renderer, &mut live_io)
            .await
            .unwrap();

    assert!(!res.is_cancelled);
    assert!(res.is_error);
    assert!(res.output.contains("failure details"));
    assert!(res.output.contains("Command exited with code 42"));
}

struct RedrawCountingTerminal {
    redraws: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl TerminalBackend for RedrawCountingTerminal {
    fn set_raw_mode(&mut self, _enabled: bool) -> io::Result<()> {
        Ok(())
    }

    fn size(&self) -> io::Result<(u16, u16)> {
        Ok((80, 24))
    }

    fn hide_cursor(&mut self) -> io::Result<()> {
        self.redraws.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }

    fn show_cursor(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn move_up(&mut self, _rows: usize) -> io::Result<()> {
        Ok(())
    }

    fn move_down(&mut self, _rows: usize) -> io::Result<()> {
        Ok(())
    }

    fn move_to_column(&mut self, _column: usize) -> io::Result<()> {
        Ok(())
    }

    fn clear_line(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn write_text(&mut self, _text: &str) -> io::Result<()> {
        Ok(())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn test_user_bash_runner_throttles_redraws_under_rapid_streaming() {
    let redraw_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let backend = RedrawCountingTerminal {
        redraws: redraw_count.clone(),
    };
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
    let (ui, mut events_rx) = crate::ui::interactive::InteractiveUi::channel();
    let mut input_reader = crate::repl::input_reader::TerminalInputReader::spawn_dummy();

    let renderer = crate::ui::TerminalRenderer::with_ui(ui);
    let mut live_io = super::LiveIo {
        controller: &mut controller,
        events: &mut events_rx,
        input: &mut input_reader,
    };

    let res = super::bash_runner::run_user_bash("seq 1 500", &renderer, &mut live_io)
        .await
        .unwrap();

    assert!(!res.is_cancelled);
    assert!(!res.is_error);
    assert!(res.output.contains("500"));

    let redraws = redraw_count.load(std::sync::atomic::Ordering::SeqCst);
    assert!(redraws > 0, "must perform at least one redraw");
    assert!(
        redraws <= 10,
        "rapid 500-line output must be throttled to <= 10 redraws, got {redraws}"
    );
}

#[tokio::test]
async fn test_user_bash_runner_streaming_updates_output_over_time() {
    let redraw_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let backend = RedrawCountingTerminal {
        redraws: redraw_count.clone(),
    };
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
    let (ui, mut events_rx) = crate::ui::interactive::InteractiveUi::channel();
    let mut input_reader = crate::repl::input_reader::TerminalInputReader::spawn_dummy();

    let renderer = crate::ui::TerminalRenderer::with_ui(ui);
    let mut live_io = super::LiveIo {
        controller: &mut controller,
        events: &mut events_rx,
        input: &mut input_reader,
    };

    let res = super::bash_runner::run_user_bash(
        "sh -c 'echo first; sleep 0.06; echo second; sleep 0.06; echo third'",
        &renderer,
        &mut live_io,
    )
    .await
    .unwrap();

    assert!(!res.is_cancelled);
    assert!(!res.is_error);
    assert!(res.output.contains("first"));
    assert!(res.output.contains("second"));
    assert!(res.output.contains("third"));

    let redraws = redraw_count.load(std::sync::atomic::Ordering::SeqCst);
    assert!(redraws >= 3, "must redraw across timed phases, got {redraws}");
    assert!(redraws <= 15, "must throttle redraws, got {redraws}");
}
