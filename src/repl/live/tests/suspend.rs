use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::mpsc;

use super::common::ModeTrackingTerminal;
use crate::repl::input_reader::TerminalInputReader;
use crate::repl::live::LiveIo;
use crate::ui::interactive::{InteractiveState, TerminalController, UiEvent};

#[tokio::test]
async fn live_io_suspend_for_suspends_and_restores() {
    let raw_mode = Arc::new(AtomicBool::new(false));
    let backend = ModeTrackingTerminal::new(Arc::clone(&raw_mode));
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
    assert!(raw_mode.load(Ordering::SeqCst));

    let (_tx, mut events) = mpsc::unbounded_channel::<UiEvent>();
    let mut input = TerminalInputReader::spawn_dummy();

    let mut io = LiveIo {
        controller: &mut controller,
        events: &mut events,
        input: &mut input,
    };

    let ran = io
        .suspend_for(|| {
            assert!(!raw_mode.load(Ordering::SeqCst));
            42
        })
        .unwrap();

    assert_eq!(ran, 42);
    assert!(raw_mode.load(Ordering::SeqCst));
}

#[tokio::test]
async fn live_io_suspend_for_async_suspends_and_restores() {
    let raw_mode = Arc::new(AtomicBool::new(false));
    let backend = ModeTrackingTerminal::new(Arc::clone(&raw_mode));
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
    assert!(raw_mode.load(Ordering::SeqCst));

    let (_tx, mut events) = mpsc::unbounded_channel::<UiEvent>();
    let mut input = TerminalInputReader::spawn_dummy();

    let mut io = LiveIo {
        controller: &mut controller,
        events: &mut events,
        input: &mut input,
    };

    let ran = io
        .suspend_for_async(|| async {
            assert!(!raw_mode.load(Ordering::SeqCst));
            "async_ok"
        })
        .await
        .unwrap();

    assert_eq!(ran, "async_ok");
    assert!(raw_mode.load(Ordering::SeqCst));
}

#[tokio::test]
async fn live_io_suspend_for_drains_stale_events() {
    let raw_mode = Arc::new(AtomicBool::new(false));
    let backend = ModeTrackingTerminal::new(Arc::clone(&raw_mode));
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();

    let (_tx, mut events) = mpsc::unbounded_channel::<UiEvent>();
    let event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('z'),
        crossterm::event::KeyModifiers::NONE,
    ));
    let mut input = TerminalInputReader::spawn_with_events(vec![event]);

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let mut io = LiveIo {
        controller: &mut controller,
        events: &mut events,
        input: &mut input,
    };

    io.suspend_for(|| {}).unwrap();

    let timeout_res = tokio::time::timeout(std::time::Duration::from_millis(30), io.input.recv()).await;
    assert!(timeout_res.is_err());
}
