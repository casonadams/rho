use super::common::HistoryTerminal;
use crate::repl::interactive::{CompletionSet, CompletionSources, InteractiveHistory};
use crate::repl::live::types::{EditorResources, IdleContext, LiveIo, LiveMessage};
use crate::ui::interactive::{InteractiveState, QueueKind, QueuedMessage, TerminalController};

async fn setup_live_session_and_engine(
    dir: &std::path::Path,
) -> (crate::repl::ReplSession, crate::engine::AgentEngine) {
    let config = rho_harness_core::config::Config {
        config_dir: dir.to_path_buf(),
        sessions_dir: dir.join("sessions"),
        ..Default::default()
    };
    let auth = crate::auth::AuthStore::default();
    let session = crate::repl::ReplSession::new(config.clone(), auth.clone(), None);
    let engine = crate::platform::agent_engine(config, auth, None).await.unwrap();
    (session, engine)
}

struct LiveFixture {
    _temp: tempfile::TempDir,
    controller: TerminalController<HistoryTerminal>,
    _ui: crate::ui::interactive::InteractiveUi,
    events: tokio::sync::mpsc::UnboundedReceiver<crate::ui::interactive::UiEvent>,
    input: crate::repl::input_reader::TerminalInputReader,
    history: InteractiveHistory,
    completions: CompletionSet,
    session: crate::repl::ReplSession,
    engine: crate::engine::AgentEngine,
}

impl LiveFixture {
    async fn new(input: crate::repl::input_reader::TerminalInputReader) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let history = InteractiveHistory::with_file(100, temp.path().join("h.txt")).unwrap();
        let (session, engine) = setup_live_session_and_engine(temp.path()).await;
        let (ui, events) = crate::ui::interactive::InteractiveUi::channel();
        Self {
            controller: TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap(),
            _ui: ui,
            events,
            input,
            history,
            completions: CompletionSet::from_sources(CompletionSources::default()),
            session,
            engine,
            _temp: temp,
        }
    }

    fn parts(
        &mut self,
    ) -> (
        LiveIo<'_, HistoryTerminal>,
        EditorResources<'_>,
        &mut crate::repl::ReplSession,
        &mut crate::engine::AgentEngine,
    ) {
        (
            LiveIo {
                controller: &mut self.controller,
                events: &mut self.events,
                input: &mut self.input,
            },
            EditorResources {
                history: &mut self.history,
                completions: &self.completions,
            },
            &mut self.session,
            &mut self.engine,
        )
    }
}

#[tokio::test]
async fn test_process_live_message_slash_exit_returns_true() {
    let mut fx = LiveFixture::new(crate::repl::input_reader::TerminalInputReader::spawn_dummy()).await;
    let (io, editor, session, engine) = fx.parts();
    let live = LiveMessage {
        io,
        editor,
        message: QueuedMessage {
            text: "/exit".to_string(),
            kind: QueueKind::Steering,
        },
    };

    assert!(session.process_live_message(engine, live).await.unwrap());
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
    let mut fx = LiveFixture::new(ctrl_d_input()).await;
    let (io, editor, session, engine) = fx.parts();
    let result = crate::repl::live::idle::read_idle_input(IdleContext {
        io,
        editor,
        session,
        engine,
    })
    .await
    .unwrap();
    assert!(result.is_none());
}

async fn run_live_loop_with(
    queued: Option<QueuedMessage>,
    input: crate::repl::input_reader::TerminalInputReader,
) -> Vec<String> {
    let mut fx = LiveFixture::new(input).await;
    if let Some(message) = queued {
        fx.controller.state_mut().push_front_queued(message);
    }
    let (io, editor, session, engine) = fx.parts();
    super::super::run_live_loop(io, editor, session, engine).await.unwrap();
    fx.history.entries().to_vec()
}

#[tokio::test]
async fn test_run_live_loop_exit_paths_return_ok() {
    let exit_message = QueuedMessage {
        text: "/exit".to_string(),
        kind: QueueKind::Steering,
    };
    let cases = [
        (
            Some(exit_message),
            crate::repl::input_reader::TerminalInputReader::spawn_dummy(),
            vec!["/exit".to_string()],
        ),
        (None, ctrl_d_input(), Vec::new()),
    ];
    for (queued, input, expected_history) in cases {
        assert_eq!(run_live_loop_with(queued, input).await, expected_history);
    }
}
