use super::common::HistoryTerminal;
use crate::repl::interactive::{CompletionSet, CompletionSources, InteractiveHistory};
use crate::repl::live::types::{EditorResources, IdleContext, LiveIo, LiveMessage};
use crate::ui::interactive::{InteractiveState, QueueKind, QueuedMessage, TerminalController};

async fn setup_live_session_and_engine(
    dir: &std::path::Path,
) -> (crate::repl::ReplSession, crate::engine::AgentEngine) {
    let config = rho_harness_core::config::Config {
        config_dir: dir.to_path_buf(),
        ..Default::default()
    };
    let auth = crate::auth::AuthStore::default();
    let session = crate::repl::ReplSession::new(config.clone(), auth.clone(), None);
    let engine = crate::platform::agent_engine(config, auth, None).await.unwrap();
    (session, engine)
}

#[tokio::test]
async fn test_process_live_message_slash_exit_returns_true() {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let (mut session, mut engine) = setup_live_session_and_engine(temp.path()).await;

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

    assert!(session.process_live_message(&mut engine, live).await.unwrap());
}

fn idle_key_input(event: crossterm::event::KeyEvent) -> crate::repl::input_reader::TerminalInputReader {
    crate::repl::input_reader::TerminalInputReader::spawn_with_events(vec![crossterm::event::Event::Key(event)])
}

fn ctrl_d_input() -> crate::repl::input_reader::TerminalInputReader {
    let key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('d'),
        crossterm::event::KeyModifiers::CONTROL,
    );
    idle_key_input(key)
}

#[tokio::test]
async fn test_ctrl_d_on_empty_editor_exits_idle_loop() {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let (mut session, mut engine) = setup_live_session_and_engine(temp.path()).await;
    let (_ui, mut events) = crate::ui::interactive::InteractiveUi::channel();
    let mut input = ctrl_d_input();
    let mut history = InteractiveHistory::with_file(100, temp.path().join("h.txt")).unwrap();
    let completions = CompletionSet::from_sources(CompletionSources::default());

    let io = LiveIo {
        controller: &mut controller,
        events: &mut events,
        input: &mut input,
    };
    let editor = EditorResources {
        history: &mut history,
        completions: &completions,
    };
    let result = crate::repl::live::idle::read_idle_input(IdleContext {
        io,
        editor,
        session: &mut session,
        engine: &mut engine,
    })
    .await
    .unwrap();
    assert!(result.is_none());
}
