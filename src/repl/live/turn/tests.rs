use std::io;

use super::input::{TurnInputContext, TurnKeyResult, handle_turn_key};
use crate::repl::coordinator::SharedSteeringQueue;
use crate::repl::interactive::{CompletionSet, InteractiveHistory};
use crate::ui::TerminalRenderer;
use crate::ui::interactive::{Activity, InteractiveState, QueueKind, RunningTool, TerminalBackend, TerminalController};
use crossterm::event::{KeyCode, KeyEvent, KeyEventState, KeyModifiers};

struct MockTerminal;

impl TerminalBackend for MockTerminal {
    fn set_raw_mode(&mut self, _: bool) -> io::Result<()> {
        Ok(())
    }
    fn size(&self) -> io::Result<(u16, u16)> {
        Ok((80, 24))
    }
    fn hide_cursor(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn show_cursor(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn move_up(&mut self, _: usize) -> io::Result<()> {
        Ok(())
    }
    fn move_down(&mut self, _: usize) -> io::Result<()> {
        Ok(())
    }
    fn move_to_column(&mut self, _: usize) -> io::Result<()> {
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

fn key_event(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent {
        code,
        modifiers,
        kind: crossterm::event::KeyEventKind::Press,
        state: KeyEventState::empty(),
    }
}

struct TurnTestFixture {
    _temp: tempfile::TempDir,
    controller: TerminalController<MockTerminal>,
    history: InteractiveHistory,
    completions: CompletionSet,
    batch: super::LiveBatch,
    steering: SharedSteeringQueue,
    session: crate::repl::ReplSession,
    model_switch: std::sync::Arc<rho_engine::engine::runner::SharedModelSwitch>,
}

impl TurnTestFixture {
    fn new(initial_text: &str) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let mut controller = TerminalController::new(MockTerminal, InteractiveState::default()).unwrap();
        controller.state_mut().editor_mut().set_text(initial_text);
        let history = InteractiveHistory::with_file(10, temp.path().join("history.txt")).unwrap();
        let completions = CompletionSet::from_sources(Default::default());
        let batch = super::LiveBatch::new();
        let steering = SharedSteeringQueue::new(crate::engine::runner::QueueMode::All);
        let session = crate::repl::ReplSession::new(
            rho_harness_core::config::Config::default(),
            crate::auth::AuthStore::default(),
            None,
        );
        let model_switch = std::sync::Arc::new(rho_engine::engine::runner::SharedModelSwitch::new());
        Self {
            _temp: temp,
            controller,
            history,
            completions,
            batch,
            steering,
            session,
            model_switch,
        }
    }

    fn context(&mut self) -> TurnInputContext<'_, MockTerminal> {
        TurnInputContext {
            controller: &mut self.controller,
            history: &mut self.history,
            completions: &self.completions,
            batch: &mut self.batch,
            steering: &self.steering,
            session: &mut self.session,
            model_switch: &self.model_switch,
            shared_auth: None,
        }
    }
}

#[tokio::test]
async fn test_turn_input_enter_queues_steering_and_sets_status() {
    let mut f = TurnTestFixture::new("steer this tool");
    let mut ctx = f.context();
    let result = handle_turn_key(key_event(KeyCode::Enter, KeyModifiers::empty()), &mut ctx)
        .await
        .unwrap();
    assert!(matches!(result, TurnKeyResult::Handled));

    let polled = crate::engine::runner::SteeringQueueProvider::poll_steering(&f.steering).await;
    assert_eq!(polled, vec!["steer this tool"]);
    assert_eq!(
        f.controller.state().system_message(),
        Some("[Steering queued for tool boundary]")
    );
    let queue = f.controller.state().queue();
    assert_eq!(
        (queue.len(), queue[0].text.as_str(), queue[0].kind),
        (1, "steer this tool", QueueKind::Steering)
    );
}

#[tokio::test]
async fn test_turn_input_alt_enter_queues_follow_up_without_steering() {
    let mut f = TurnTestFixture::new("run after turn");
    let mut ctx = f.context();
    let result = handle_turn_key(key_event(KeyCode::Enter, KeyModifiers::ALT), &mut ctx)
        .await
        .unwrap();
    assert!(matches!(result, TurnKeyResult::Handled));

    let polled = crate::engine::runner::SteeringQueueProvider::poll_steering(&f.steering).await;
    assert!(polled.is_empty());
    assert_eq!(
        f.controller.state().system_message(),
        Some("[Follow-up queued for turn completion]")
    );
    let queue = f.controller.state().queue();
    assert_eq!(
        (queue.len(), queue[0].text.as_str(), queue[0].kind),
        (1, "run after turn", QueueKind::FollowUp)
    );
}

#[tokio::test]
async fn test_turn_input_multiple_queued_messages() {
    let mut f = TurnTestFixture::new("steer 1");
    let mut ctx = f.context();
    let _ = handle_turn_key(key_event(KeyCode::Enter, KeyModifiers::empty()), &mut ctx)
        .await
        .unwrap();

    f.controller.state_mut().editor_mut().set_text("steer 2");
    let mut ctx = f.context();
    let _ = handle_turn_key(key_event(KeyCode::Enter, KeyModifiers::empty()), &mut ctx)
        .await
        .unwrap();

    f.controller.state_mut().editor_mut().set_text("follow up");
    let mut ctx = f.context();
    let _ = handle_turn_key(key_event(KeyCode::Enter, KeyModifiers::ALT), &mut ctx)
        .await
        .unwrap();

    let queue = f.controller.state().queue();
    assert_eq!(
        (
            queue.len(),
            queue[0].text.as_str(),
            queue[1].text.as_str(),
            queue[2].text.as_str()
        ),
        (3, "steer 1", "steer 2", "follow up")
    );
}

#[tokio::test]
async fn test_turn_input_escape_cancels() {
    let mut f = TurnTestFixture::new("");
    let mut ctx = f.context();
    let result = handle_turn_key(key_event(KeyCode::Esc, KeyModifiers::empty()), &mut ctx)
        .await
        .unwrap();
    assert!(matches!(result, TurnKeyResult::Cancelled));
}

#[tokio::test]
async fn test_turn_input_ctrl_c_clears_input_without_cancelling() {
    let mut f = TurnTestFixture::new("partial input to discard");
    let mut ctx = f.context();
    let result = handle_turn_key(key_event(KeyCode::Char('c'), KeyModifiers::CONTROL), &mut ctx)
        .await
        .unwrap();
    assert!(matches!(result, TurnKeyResult::Handled));
    assert_eq!(f.controller.state().editor().text(), "");
}

#[tokio::test]
async fn test_turn_input_ctrl_l_opens_model_selector() {
    let mut f = TurnTestFixture::new("");
    let mut ctx = f.context();
    let result = handle_turn_key(key_event(KeyCode::Char('l'), KeyModifiers::CONTROL), &mut ctx)
        .await
        .unwrap();
    assert!(matches!(result, TurnKeyResult::Handled));
    assert_eq!(f.controller.state().active_modal().unwrap().title, "Select Model");
}

fn model_switch_fixture() -> (
    TerminalController<MockTerminal>,
    super::LiveBatch,
    rho_harness_core::config::Config,
    crate::auth::AuthStore,
    TerminalRenderer,
    std::sync::Arc<rho_engine::engine::runner::SharedModelSwitch>,
) {
    (
        TerminalController::new(MockTerminal, InteractiveState::default()).unwrap(),
        super::LiveBatch::new(),
        rho_harness_core::config::Config::default(),
        crate::auth::AuthStore::default(),
        TerminalRenderer::default(),
        std::sync::Arc::new(rho_engine::engine::runner::SharedModelSwitch::new()),
    )
}

#[tokio::test]
async fn test_apply_turn_model_switch_updates_model_switch_and_footer() {
    let (mut controller, mut batch, mut config, auth_store, renderer, model_switch) = model_switch_fixture();
    let input = super::TurnModelSwitchInput {
        model: "llama3.2",
        provider: "local",
        save_as_default: false,
        config: &mut config,
        auth_store: &auth_store,
        renderer: &renderer,
        controller: &mut controller,
        model_switch: &model_switch,
        batch: &mut batch,
        shared_auth: None,
    };
    super::apply_turn_model_switch(input).await.unwrap();
    assert_eq!((config.model.as_str(), config.provider.as_str()), ("llama3.2", "local"));
    assert_eq!(
        (
            model_switch.current_model().as_deref(),
            controller.state().footer().model.as_str()
        ),
        (Some("llama3.2"), "llama3.2")
    );
}

#[tokio::test]
async fn test_turn_input_cycle_model_shortcut() {
    let mut f = TurnTestFixture::new("");
    let mut ctx = f.context();
    let result = handle_turn_key(key_event(KeyCode::Char('p'), KeyModifiers::CONTROL), &mut ctx)
        .await
        .unwrap();
    assert!(matches!(result, TurnKeyResult::Handled));
}

struct ActiveTurnHarness {
    _temp: tempfile::TempDir,
    session: crate::repl::ReplSession,
    engine: crate::engine::AgentEngine,
    controller: TerminalController<MockTerminal>,
    ui_events: tokio::sync::mpsc::UnboundedReceiver<crate::ui::interactive::UiEvent>,
    input_reader: crate::repl::input_reader::TerminalInputReader,
    history: InteractiveHistory,
    completions: CompletionSet,
}

async fn create_harness_engine(
    temp: &std::path::Path,
) -> (
    rho_harness_core::config::Config,
    crate::auth::AuthStore,
    crate::engine::AgentEngine,
) {
    let config = rho_harness_core::config::Config {
        provider: "local".to_string(),
        model: "llama3.2".to_string(),
        sessions_dir: temp.join("sessions"),
        ..Default::default()
    };
    let auth = crate::auth::AuthStore::default();
    let engine = crate::engine::builder::AgentEngineBuilder::new(config.clone(), auth.clone())
        .build()
        .await
        .unwrap();
    (config, auth, engine)
}

impl ActiveTurnHarness {
    async fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let (config, auth, engine) = create_harness_engine(temp.path()).await;
        let mut session = crate::repl::ReplSession::new(config, auth, None);
        let (ui, ui_events) = crate::ui::interactive::InteractiveUi::channel();
        session.renderer = TerminalRenderer::with_ui(ui);
        let controller = TerminalController::new(MockTerminal, InteractiveState::default()).unwrap();
        let cancel = crossterm::event::Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::empty()));
        let input_reader = crate::repl::input_reader::TerminalInputReader::spawn_with_events(vec![cancel]);
        let history = InteractiveHistory::with_file(10, temp.path().join("history.txt")).unwrap();
        let completions = CompletionSet::from_sources(Default::default());
        Self {
            _temp: temp,
            session,
            engine,
            controller,
            ui_events,
            input_reader,
            history,
            completions,
        }
    }

    async fn run_turn(&mut self, prompt: &str) {
        let turn = crate::repl::live::ActiveTurn {
            io: crate::repl::live::LiveIo {
                controller: &mut self.controller,
                events: &mut self.ui_events,
                input: &mut self.input_reader,
            },
            editor: crate::repl::live::EditorResources {
                history: &mut self.history,
                completions: &self.completions,
            },
            prompt,
        };
        super::run_active_turn(&mut self.session, &self.engine, turn)
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn test_turn_cancellation_clears_active_tool_and_idle_footer() {
    let mut h = ActiveTurnHarness::new().await;
    h.controller.state_mut().footer_mut().activity = Activity::Working;
    h.controller.state_mut().footer_mut().running_tool = Some("bash".to_string());
    h.controller
        .state_mut()
        .set_active_tool(Some(RunningTool::new("bash".to_string(), "sleep 30".to_string(), None)));

    h.run_turn("sleep 30").await;
    let state = (
        h.controller.state().footer().activity == Activity::Idle,
        h.controller.state().footer().running_tool.clone(),
    );
    assert_eq!(state, (true, None));
    assert!(h.controller.state().active_tool().is_none());
}

#[tokio::test]
async fn test_turn_with_active_modal_advances_spinner() {
    let mut h = ActiveTurnHarness::new().await;
    h.controller.state_mut().footer_mut().activity = Activity::Working;
    h.controller
        .state_mut()
        .push_modal(crate::ui::interactive::ModalState::new("Select Model", "", vec![]));

    h.run_turn("test").await;
}

#[tokio::test]
async fn test_turn_finish_active_turn_handles_compacted_notice() {
    let mut h = ActiveTurnHarness::new().await;
    let out = crate::engine::runner::TurnOutput {
        final_text: String::new(),
        tool_calls_count: 0,
        tool_failures_count: 0,
        requests: 0,
        usage: None,
        status: crate::engine::runner::RunStatus::Compacted,
        metrics: rho_engine::engine::metrics::RunMetrics::default(),
    };
    let steering = std::sync::Arc::new(crate::repl::coordinator::SharedSteeringQueue::new(
        h.engine.config.steering_mode,
    ));
    let model_switch = std::sync::Arc::new(rho_engine::engine::runner::SharedModelSwitch::new());
    let mut loop_ctx =
        super::runner::TurnLoop::new((&mut h.session, &h.engine), &mut h.controller, (steering, model_switch));
    super::cancel::finish_active_turn(&mut loop_ctx, &mut h.ui_events, Ok(out)).unwrap();
    assert_eq!(h.controller.state().footer().activity, Activity::Idle);
}

#[tokio::test]
async fn test_turn_paste_routes_to_active_modal() {
    let temp = tempfile::tempdir().unwrap();
    let (_, _, engine) = create_harness_engine(temp.path()).await;
    let mut f = TurnTestFixture::new("background text");
    let mut modal = crate::ui::interactive::ModalState::new("Permission Required", "", vec![]);
    modal.enter_input_mode("edit");
    f.controller.state_mut().push_modal(modal);

    let steering = std::sync::Arc::new(f.steering.clone());
    let mut lp = super::runner::TurnLoop::new(
        (&mut f.session, &engine),
        &mut f.controller,
        (steering, f.model_switch.clone()),
    );
    let (_tx, mut ui_events) = tokio::sync::mpsc::unbounded_channel();
    let cancellation = crate::engine::runner::CancellationSignal::default();
    let mut res = super::event::TurnInputResources {
        history: &mut f.history,
        completions: &f.completions,
        ui_events: &mut ui_events,
        cancellation: &cancellation,
    };

    let handled = super::event::dispatch_turn_input(
        &mut lp,
        &mut res,
        crossterm::event::Event::Paste("pasted command".to_string()),
    )
    .await
    .unwrap();
    assert!(!handled);
    assert_eq!(
        f.controller.state().active_modal().unwrap().input.text(),
        "pasted command"
    );
    f.controller.state_mut().pop_modal();
    assert_eq!(f.controller.state().editor().text(), "background text");
}

#[tokio::test]
async fn test_turn_focus_events_toggle_focused_state() {
    let temp = tempfile::tempdir().unwrap();
    let (_, _, engine) = create_harness_engine(temp.path()).await;
    let mut f = TurnTestFixture::new("text");
    assert!(f.controller.focused());

    let steering = std::sync::Arc::new(f.steering.clone());
    let mut lp = super::runner::TurnLoop::new(
        (&mut f.session, &engine),
        &mut f.controller,
        (steering, f.model_switch.clone()),
    );
    let (_tx, mut ui_events) = tokio::sync::mpsc::unbounded_channel();
    let cancellation = crate::engine::runner::CancellationSignal::default();
    let mut res = super::event::TurnInputResources {
        history: &mut f.history,
        completions: &f.completions,
        ui_events: &mut ui_events,
        cancellation: &cancellation,
    };

    lp.controller.state_mut().footer_mut().activity = Activity::Working;
    super::event::dispatch_turn_input(&mut lp, &mut res, crossterm::event::Event::FocusLost)
        .await
        .unwrap();
    assert!(!lp.controller.focused());
    let unfocused_layout = lp.controller.rendered().unwrap();
    assert!(unfocused_layout.working_line.contains("Working..."));
    assert!(!unfocused_layout.cursor_visible);

    super::event::dispatch_turn_input(&mut lp, &mut res, crossterm::event::Event::FocusGained)
        .await
        .unwrap();
    assert!(lp.controller.focused());
    let focused_layout = lp.controller.rendered().unwrap();
    assert!(focused_layout.working_line.contains("Working..."));
    assert!(focused_layout.cursor_visible);
}
