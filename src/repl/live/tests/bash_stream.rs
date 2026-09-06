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

struct TestBashStreamFixture {
    controller: TerminalController<RedrawCountingTerminal>,
    events_rx: tokio::sync::mpsc::UnboundedReceiver<crate::ui::interactive::UiEvent>,
    input_reader: crate::repl::input_reader::TerminalInputReader,
    renderer: crate::ui::TerminalRenderer,
    redraw_count: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

fn setup_bash_stream_fixture() -> TestBashStreamFixture {
    let redraw_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let backend = RedrawCountingTerminal {
        redraws: redraw_count.clone(),
    };
    let controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
    let (ui, events_rx) = crate::ui::interactive::InteractiveUi::channel();
    let input_reader = crate::repl::input_reader::TerminalInputReader::spawn_dummy();
    let renderer = crate::ui::TerminalRenderer::with_ui(ui);
    TestBashStreamFixture {
        controller,
        events_rx,
        input_reader,
        renderer,
        redraw_count,
    }
}

#[tokio::test]
async fn test_user_bash_runner_streaming_updates_output_over_time() {
    let mut f = setup_bash_stream_fixture();
    let mut live_io = super::super::LiveIo {
        controller: &mut f.controller,
        events: &mut f.events_rx,
        input: &mut f.input_reader,
    };

    let res = super::super::bash_runner::run_user_bash(
        "sh -c 'echo first; sleep 0.06; echo second; sleep 0.06; echo third'",
        &f.renderer,
        &mut live_io,
    )
    .await
    .unwrap();

    assert!(!res.is_cancelled && !res.is_error);
    for word in ["first", "second", "third"] {
        assert!(res.output.contains(word));
    }

    let redraws = f.redraw_count.load(std::sync::atomic::Ordering::SeqCst);
    assert!((3..=15).contains(&redraws));
}
