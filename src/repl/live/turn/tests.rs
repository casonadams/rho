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
        Self {
            _temp: temp,
            controller,
            history,
            completions,
            batch,
            steering,
            session,
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
            shared_auth: None,
        }
    }
}

struct LoopFixture {
    _temp: tempfile::TempDir,
    engine: crate::engine::AgentEngine,
    base: TurnTestFixture,
    tx: tokio::sync::mpsc::UnboundedSender<crate::ui::interactive::UiEvent>,
    ui_events: tokio::sync::mpsc::UnboundedReceiver<crate::ui::interactive::UiEvent>,
    cancellation: crate::engine::runner::CancellationSignal,
    input_reader: crate::repl::input_reader::TerminalInputReader,
}

impl LoopFixture {
    async fn new(initial_text: &str) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let (_, _, engine) = create_harness_engine(temp.path()).await;
        let (tx, ui_events) = tokio::sync::mpsc::unbounded_channel();
        Self {
            _temp: temp,
            engine,
            base: TurnTestFixture::new(initial_text),
            tx,
            ui_events,
            cancellation: crate::engine::runner::CancellationSignal::default(),
            input_reader: crate::repl::input_reader::TerminalInputReader::spawn_dummy(),
        }
    }

    fn context(&mut self) -> super::TurnContext<'_, MockTerminal> {
        let steering = std::sync::Arc::new(self.base.steering.clone());
        super::TurnContext {
            loop_ctx: super::runner::TurnLoop::new(
                &mut self.base.session,
                &self.engine,
                &mut self.base.controller,
                steering,
            ),
            input_reader: &mut self.input_reader,
            resources: super::event::TurnInputResources {
                history: &mut self.base.history,
                completions: &self.base.completions,
                ui_events: &mut self.ui_events,
                cancellation: &self.cancellation,
            },
        }
    }
}

async fn dispatch(ctx: &mut super::TurnContext<'_, MockTerminal>, event: crossterm::event::Event) -> bool {
    super::event::dispatch_turn_input(&mut ctx.loop_ctx, &mut ctx.resources, event)
        .await
        .unwrap()
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
) {
    (
        TerminalController::new(MockTerminal, InteractiveState::default()).unwrap(),
        super::LiveBatch::new(),
        rho_harness_core::config::Config::default(),
        crate::auth::AuthStore::default(),
    )
}

#[tokio::test]
async fn test_apply_turn_model_switch_updates_config_and_footer() {
    let cases = [
        ("local", false, false),
        ("local", true, false),
        ("no-such-provider", false, true),
    ];
    for (provider, shared, expect_warning) in cases {
        let (mut controller, mut batch, mut config, auth_store) = model_switch_fixture();
        let (ui, mut ui_events) = crate::ui::interactive::InteractiveUi::channel();
        let renderer = TerminalRenderer::with_ui(ui);
        let input = super::TurnModelSwitchInput {
            model: "llama3.2",
            provider,
            save_as_default: false,
            config: &mut config,
            auth_store: &auth_store,
            renderer: &renderer,
            controller: &mut controller,
            batch: &mut batch,
            shared_auth: shared.then(|| std::sync::Arc::new(tokio::sync::Mutex::new(auth_store.clone()))),
        };
        super::apply_turn_model_switch(input).await.unwrap();
        assert_eq!(
            (config.model.as_str(), config.provider.as_str()),
            ("llama3.2", provider)
        );
        assert_eq!(controller.state().footer().model.as_str(), "llama3.2");
        assert_eq!(controller.state().footer().provider.as_str(), provider);
        let warned = std::iter::from_fn(|| ui_events.try_recv().ok()).any(|event| {
            matches!(
                event,
                crate::ui::interactive::UiEvent::Transcript(crate::ui::interactive::TranscriptItem::Notice(text))
                    if text.contains("Could not switch model")
            )
        });
        assert_eq!(warned, expect_warning);
    }
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

#[tokio::test]
async fn test_turn_input_display_toggles() {
    let mut f = TurnTestFixture::new("");
    let mut ctx = f.context();

    assert!(!ctx.controller.tools_expanded());
    let result = handle_turn_key(key_event(KeyCode::Char('o'), KeyModifiers::CONTROL), &mut ctx)
        .await
        .unwrap();
    assert!(matches!(result, TurnKeyResult::Handled));
    assert!(ctx.controller.tools_expanded());

    let result = handle_turn_key(key_event(KeyCode::Char('o'), KeyModifiers::CONTROL), &mut ctx)
        .await
        .unwrap();
    assert!(matches!(result, TurnKeyResult::Handled));
    assert!(!ctx.controller.tools_expanded());

    assert!(!ctx.controller.hide_thinking());
    let result = handle_turn_key(key_event(KeyCode::Char('t'), KeyModifiers::CONTROL), &mut ctx)
        .await
        .unwrap();
    assert!(matches!(result, TurnKeyResult::Handled));
    assert!(ctx.controller.hide_thinking());

    let result = handle_turn_key(key_event(KeyCode::Char('t'), KeyModifiers::CONTROL), &mut ctx)
        .await
        .unwrap();
    assert!(matches!(result, TurnKeyResult::Handled));
    assert!(!ctx.controller.hide_thinking());

    super::input::handle_display_toggle(ctx.controller, &crate::ui::interactive::InputAction::Clear);
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
async fn test_turn_cancellation_during_thinking_preserves_partial_thinking_in_transcript() {
    let mut h = ActiveTurnHarness::new().await;
    let cancellation = crate::engine::runner::CancellationSignal::default();
    let steering = std::sync::Arc::new(crate::repl::coordinator::SharedSteeringQueue::new(
        crate::engine::runner::QueueMode::OneAtATime,
    ));
    let mut loop_ctx = super::runner::TurnLoop::new(&mut h.session, &h.engine, &mut h.controller, steering);

    loop_ctx.session.renderer.finish_thinking("partial ponderings");

    super::cancel::cancel_active_turn(&mut loop_ctx, &mut h.ui_events, &cancellation)
        .await
        .unwrap();

    assert!(
        loop_ctx.controller.transcript().iter().any(
            |item| matches!(item, crate::ui::interactive::TranscriptItem::Thinking(t) if t == "partial ponderings")
        ),
        "partial thinking must be committed to transcript on cancellation"
    );
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
    assert_finish_turn_goes_idle(crate::engine::runner::RunStatus::Compacted).await;
}

#[tokio::test]
async fn test_turn_paste_routes_to_active_modal() {
    let mut f = LoopFixture::new("background text").await;
    let mut modal = crate::ui::interactive::ModalState::new("Permission Required", "", vec![]);
    modal.enter_input_mode("edit");
    f.base.controller.state_mut().push_modal(modal);

    let mut ctx = f.context();
    let handled = dispatch(&mut ctx, crossterm::event::Event::Paste("pasted command".to_string())).await;
    assert!(!handled);
    assert_eq!(
        ctx.loop_ctx.controller.state().active_modal().unwrap().input.text(),
        "pasted command"
    );
    ctx.loop_ctx.controller.state_mut().pop_modal();
    assert_eq!(ctx.loop_ctx.controller.state().editor().text(), "background text");
}

#[tokio::test]
async fn test_turn_focus_events_toggle_focused_state() {
    let mut f = LoopFixture::new("text").await;
    assert!(f.base.controller.focused());

    let mut ctx = f.context();
    dispatch(&mut ctx, crossterm::event::Event::FocusLost).await;
    assert!(!ctx.loop_ctx.controller.focused());
    let unfocused_layout = ctx.loop_ctx.controller.rendered().unwrap();
    assert!(unfocused_layout.working_line.contains("working"));
    assert!(unfocused_layout.lines.iter().any(|l| l.contains("working")));
    assert!(!unfocused_layout.cursor_visible);

    dispatch(&mut ctx, crossterm::event::Event::FocusGained).await;
    assert!(ctx.loop_ctx.controller.focused());
    let focused_layout = ctx.loop_ctx.controller.rendered().unwrap();
    assert!(focused_layout.working_line.contains("working"));
    assert!(focused_layout.cursor_visible);
}

#[tokio::test]
async fn test_turn_idle_activity_event_during_active_turn_does_not_hide_working_line() {
    let mut f = LoopFixture::new("text").await;
    f.tx.send(crate::ui::interactive::UiEvent::Activity(Activity::Idle))
        .unwrap();

    let mut ctx = f.context();
    assert_eq!(ctx.loop_ctx.controller.state().footer().activity, Activity::Working);
    ctx.loop_ctx.drain_ui_batch(ctx.resources.ui_events, false).unwrap();
    assert_eq!(ctx.loop_ctx.controller.state().footer().activity, Activity::Working);

    dispatch(&mut ctx, crossterm::event::Event::FocusLost).await;
    assert!(!ctx.loop_ctx.controller.focused());
    let unfocused_layout = ctx.loop_ctx.controller.rendered().unwrap();
    assert!(unfocused_layout.working_line.contains("working"));
    assert!(!unfocused_layout.cursor_visible);
}

#[tokio::test]
async fn test_turn_resize_and_editor_paste_events() {
    let mut f = LoopFixture::new("initial text").await;
    let mut ctx = f.context();

    let handled = dispatch(&mut ctx, crossterm::event::Event::Resize(100, 30)).await;
    assert!(!handled);
    assert_eq!(ctx.loop_ctx.controller.width(), 100);

    let handled = dispatch(&mut ctx, crossterm::event::Event::Paste(" appended".to_string())).await;
    assert!(!handled);
    assert_eq!(ctx.loop_ctx.controller.state().editor().text(), "initial text appended");

    ctx.loop_ctx.controller.state_mut().footer_mut().activity = Activity::Idle;
    dispatch(&mut ctx, crossterm::event::Event::FocusLost).await;
    assert!(!ctx.loop_ctx.controller.focused());
    assert_eq!(ctx.loop_ctx.controller.state().footer().activity, Activity::Working);

    dispatch(&mut ctx, crossterm::event::Event::FocusLost).await;

    let handled = dispatch(
        &mut ctx,
        crossterm::event::Event::Mouse(crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Moved,
            column: 0,
            row: 0,
            modifiers: crossterm::event::KeyModifiers::NONE,
        }),
    )
    .await;
    assert!(!handled);
}

#[tokio::test]
async fn test_turn_ui_transcript_event_flushes_immediately() {
    let mut f = LoopFixture::new("text").await;
    let mut ctx = f.context();

    let tool_item = rho_harness_core::presentation::ToolLine {
        name: "write".to_string(),
        arguments: serde_json::json!({"path": "foo.txt", "content": "bar"}),
        output: "ok".to_string(),
        output_summary: "ok".to_string(),
        is_error: false,
        duration_ms: None,
    };
    let event = super::TurnEvent::Ui(crate::ui::interactive::UiEvent::Transcript(
        crate::ui::interactive::TranscriptItem::Tool(tool_item),
    ));

    super::handle_turn_event(&mut ctx, event).await.unwrap();

    assert_eq!(ctx.loop_ctx.controller.transcript().len(), 1);
    assert!(ctx.loop_ctx.batch.ui.is_empty());
}

async fn assert_finish_turn_goes_idle(status: crate::engine::runner::RunStatus) {
    let mut h = ActiveTurnHarness::new().await;
    let out = crate::engine::runner::TurnOutput {
        final_text: String::new(),
        tool_calls_count: 0,
        tool_failures_count: 0,
        requests: 0,
        usage: None,
        status,
        metrics: rho_engine::engine::metrics::RunMetrics::default(),
    };
    let steering = std::sync::Arc::new(crate::repl::coordinator::SharedSteeringQueue::new(
        h.engine.config.steering_mode,
    ));
    let mut loop_ctx = super::runner::TurnLoop::new(&mut h.session, &h.engine, &mut h.controller, steering);
    super::cancel::finish_active_turn(&mut loop_ctx, &mut h.ui_events, Ok(out)).unwrap();
    assert_eq!(h.controller.state().footer().activity, Activity::Idle);
}

#[tokio::test]
async fn test_turn_finish_active_turn_invalidates_and_refreshes_quota() {
    assert_finish_turn_goes_idle(crate::engine::runner::RunStatus::Completed).await;
}

#[tokio::test]
async fn test_turn_quota_periodic_and_updated_events_sync_footer() {
    let mut f = LoopFixture::new("text").await;
    let mut ctx = f.context();

    let result = super::handle_turn_event(&mut ctx, super::TurnEvent::QuotaPeriodic).await;
    assert!(result.is_ok());
    assert!(!result.unwrap());

    let result = super::handle_turn_event(&mut ctx, super::TurnEvent::QuotaUpdated).await;
    assert!(result.is_ok());
    assert!(!result.unwrap());
}

#[tokio::test]
async fn test_turn_loop_tick_retires_consumed_steering_from_queue() {
    let mut f = LoopFixture::new("steer this tool").await;
    let mut ctx = f.context();
    dispatch(
        &mut ctx,
        crossterm::event::Event::Key(key_event(KeyCode::Enter, KeyModifiers::empty())),
    )
    .await;
    assert_eq!(ctx.loop_ctx.controller.state().queue().len(), 1);

    crate::engine::runner::SteeringQueueProvider::poll_steering(&*ctx.loop_ctx.steering).await;
    ctx.loop_ctx.on_tick().unwrap();
    assert_eq!(ctx.loop_ctx.controller.state().queue().len(), 0);
}

#[tokio::test]
async fn test_turn_loop_tick_refreshes_spinner_only_after_frame_interval() {
    let cases = [(false, false), (true, true)];
    for (elapsed, refreshed) in cases {
        let mut f = LoopFixture::new("").await;
        let mut ctx = f.context();
        let before = if elapsed {
            std::time::Instant::now() - crate::repl::live::batch::SPINNER_FRAME_INTERVAL
        } else {
            std::time::Instant::now()
        };
        ctx.loop_ctx.last_spinner = before;
        ctx.loop_ctx.on_tick().unwrap();
        assert_eq!(ctx.loop_ctx.last_spinner != before, refreshed);
    }
}
