use super::common::RedrawCountingTerminal;
use crate::ui::interactive::{InteractiveState, TerminalController};

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
    let mut live_io = super::super::LiveIo {
        controller: &mut controller,
        events: &mut events_rx,
        input: &mut input_reader,
    };

    let res = super::super::bash_runner::run_user_bash("seq 1 500", &renderer, &mut live_io)
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
    let mut live_io = super::super::LiveIo {
        controller: &mut controller,
        events: &mut events_rx,
        input: &mut input_reader,
    };

    let res = super::super::bash_runner::run_user_bash(
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
