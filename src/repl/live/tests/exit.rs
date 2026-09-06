use super::common::HistoryTerminal;
use crate::repl::interactive::{CompletionSet, CompletionSources, InteractiveHistory};
use crate::repl::live::types::{EditorResources, IdleContext, LiveIo, LiveMessage};
use crate::ui::interactive::{InteractiveState, QueueKind, QueuedMessage, TerminalController};

#[tokio::test]
async fn test_process_live_message_slash_exit_returns_true() {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let config = rho_harness_core::config::Config {
        config_dir: temp.path().to_path_buf(),
        ..Default::default()
    };
    let auth_store = crate::auth::AuthStore::default();
    let mut session = crate::repl::ReplSession::new(config.clone(), auth_store.clone(), None);
    let mut engine = crate::platform::agent_engine(config, auth_store, None).await.unwrap();

    let (_ui, mut events) = crate::ui::interactive::InteractiveUi::channel();
    let mut input = crate::repl::input_reader::TerminalInputReader::spawn_dummy();
    let mut history = InteractiveHistory::with_file(100, temp.path().join("h.txt")).unwrap();
    let completions = CompletionSet::from_sources(CompletionSources::default());

    let live = LiveMessage {
        io: LiveIo {
            controller: &mut controller,
            events: &mut events,
            input: &mut input,
        },
        editor: EditorResources {
            history: &mut history,
            completions: &completions,
        },
        message: QueuedMessage {
            text: "/exit".to_string(),
            kind: QueueKind::Steering,
        },
    };

    let done = session.process_live_message(&mut engine, live).await.unwrap();
    assert!(done, "/exit must return true to terminate the live session loop");
}

#[tokio::test]
async fn test_ctrl_d_on_empty_editor_exits_idle_loop() {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let config = rho_harness_core::config::Config {
        config_dir: temp.path().to_path_buf(),
        ..Default::default()
    };
    let auth_store = crate::auth::AuthStore::default();
    let mut session = crate::repl::ReplSession::new(config.clone(), auth_store.clone(), None);
    let mut engine = crate::platform::agent_engine(config, auth_store, None).await.unwrap();

    let (_ui, mut events) = crate::ui::interactive::InteractiveUi::channel();
    let ctrl_d = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('d'),
        crossterm::event::KeyModifiers::CONTROL,
    );
    let mut input =
        crate::repl::input_reader::TerminalInputReader::spawn_with_events(vec![crossterm::event::Event::Key(ctrl_d)]);
    let mut history = InteractiveHistory::with_file(100, temp.path().join("h.txt")).unwrap();
    let completions = CompletionSet::from_sources(CompletionSources::default());

    let result = crate::repl::live::idle::read_idle_input(IdleContext {
        io: LiveIo {
            controller: &mut controller,
            events: &mut events,
            input: &mut input,
        },
        editor: EditorResources {
            history: &mut history,
            completions: &completions,
        },
        session: &mut session,
        engine: &mut engine,
    })
    .await
    .unwrap();

    assert!(
        result.is_none(),
        "Ctrl+D on an empty editor must return None to signal exit"
    );
}
