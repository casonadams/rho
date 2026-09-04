use super::common::HistoryTerminal;
use crate::ui::interactive::{InteractiveState, TerminalController};

#[tokio::test]
async fn test_user_bash_runner_streams_and_completes() {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    let (_events_tx, mut events_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut input_reader = crate::repl::input_reader::TerminalInputReader::spawn_dummy();

    let renderer = crate::ui::TerminalRenderer::default();
    let mut live_io = super::super::LiveIo {
        controller: &mut controller,
        events: &mut events_rx,
        input: &mut input_reader,
    };

    let res = super::super::bash_runner::run_user_bash("echo 'hello from user bash'", &renderer, &mut live_io)
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
    let mut live_io = super::super::LiveIo {
        controller: &mut controller,
        events: &mut events_rx,
        input: &mut input_reader,
    };

    let res = super::super::bash_runner::run_user_bash("sleep 30 & wait", &renderer, &mut live_io)
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
    let mut live_io = super::super::LiveIo {
        controller: &mut controller,
        events: &mut events_rx,
        input: &mut input_reader,
    };

    let res = super::super::bash_runner::run_user_bash("seq 1 2500", &renderer, &mut live_io)
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
    let mut live_io = super::super::LiveIo {
        controller: &mut controller,
        events: &mut events_rx,
        input: &mut input_reader,
    };

    let res = super::super::bash_runner::run_user_bash(
        "sh -c 'echo \"failure details\" >&2; exit 42'",
        &renderer,
        &mut live_io,
    )
    .await
    .unwrap();

    assert!(!res.is_cancelled);
    assert!(res.is_error);
    assert!(res.output.contains("failure details"));
    assert!(res.output.contains("Command exited with code 42"));
}
