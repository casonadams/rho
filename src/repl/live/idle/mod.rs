pub(crate) mod dispatch;
pub(crate) mod modal_action;
pub(crate) mod shortcut;

use dispatch::{IdleInputResult, process_raw_input};

use super::batch::{LiveBatch, OUTPUT_FRAME_INTERVAL};
use super::types::{FrameOrUi, next_frame_or_ui, wait_quota};
use super::{EditorResources, IdleContext, LiveIo};
use crate::error::Result;
use crate::repl::ReplSession;
use crate::ui::interactive::{QueuedMessage, TerminalBackend, TerminalController};
use crossterm::event::Event;

type UiEventReceiver = tokio::sync::mpsc::UnboundedReceiver<crate::ui::interactive::UiEvent>;

pub(crate) struct LiveIdleContext<'a, 'b> {
    pub session: &'b mut ReplSession,
    pub engine: &'b mut crate::engine::AgentEngine,
    pub last_escape_time: &'a mut Option<std::time::Instant>,
}

struct IdleUi {
    batch: LiveBatch,
    frame: tokio::time::Interval,
    quota_rx: tokio::sync::watch::Receiver<u64>,
    bg_rx: tokio::sync::broadcast::Receiver<String>,
    mcp: Option<std::sync::Arc<rho_engine::mcp::McpRuntime>>,
}

impl IdleUi {
    fn new(engine: &crate::engine::AgentEngine) -> Self {
        let mut frame = tokio::time::interval(OUTPUT_FRAME_INTERVAL);
        frame.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        Self {
            batch: LiveBatch::new(),
            frame,
            quota_rx: engine.quota_subscribe(),
            bg_rx: crate::engine::runner::turn::types::subscribe_background_notifications(),
            mcp: engine.mcp_runtime().cloned(),
        }
    }
}

enum IdleTick {
    Frame,
    Quota,
    Ui(crate::ui::interactive::UiEvent),
}

impl From<FrameOrUi> for IdleTick {
    fn from(tick: FrameOrUi) -> Self {
        match tick {
            FrameOrUi::Frame => IdleTick::Frame,
            FrameOrUi::Ui(event) => IdleTick::Ui(event),
        }
    }
}

enum IdleSource {
    Tick(IdleTick),
    Input(Option<std::io::Result<Event>>),
    BackgroundNotification(Option<String>),
    McpProblems,
}

async fn wait_mcp_problems(runtime: Option<&rho_engine::mcp::McpRuntime>) {
    match runtime {
        Some(runtime) => runtime.problems_ready().await,
        None => std::future::pending().await,
    }
}

/// The session a notice was queued for, or `None` when the channel overflowed and wake-ups were
/// dropped, in which case this session's queue must be checked anyway.
async fn wait_bg_notif(rx: &mut tokio::sync::broadcast::Receiver<String>) -> Option<Option<String>> {
    match rx.recv().await {
        Ok(owner) => Some(Some(owner)),
        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => Some(None),
        Err(tokio::sync::broadcast::error::RecvError::Closed) => None,
    }
}

async fn next_idle_step(
    ui: &mut IdleUi,
    input: &mut super::TerminalInputReader,
    ui_events: &mut UiEventReceiver,
) -> IdleSource {
    tokio::select! {
        biased;
        event = input.recv() => IdleSource::Input(event),
        Some(notif) = wait_bg_notif(&mut ui.bg_rx) => IdleSource::BackgroundNotification(notif),
        () = wait_mcp_problems(ui.mcp.as_deref()) => IdleSource::McpProblems,
        _ = wait_quota(&mut ui.quota_rx) => IdleSource::Tick(IdleTick::Quota),
        tick = next_frame_or_ui(&mut ui.frame, ui_events) => IdleSource::Tick(tick.into()),
    }
}

pub(crate) fn sync_idle_quota<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    batch: &mut LiveBatch,
    engine: &crate::engine::AgentEngine,
) -> Result<bool> {
    let quota = engine.quota_display();
    if controller.state().footer().quota != quota {
        controller.state_mut().footer_mut().quota = quota;
        batch.flush(controller, true)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

async fn handle_ui_event<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    batch: &mut LiveBatch,
    event: crate::ui::interactive::UiEvent,
) -> Result<()> {
    batch.enqueue(controller, event)
}

fn handle_frame_tick<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    batch: &mut LiveBatch,
    session: &mut ReplSession,
    engine: &crate::engine::AgentEngine,
) -> Result<()> {
    let expired = controller.check_system_message_expiration();
    let resized = controller.refresh_size()?;
    if resized {
        session.renderer.set_width(controller.width());
    }
    let quota = engine.quota_display();
    let quota_changed = controller.state().footer().quota != quota;
    if quota_changed {
        controller.state_mut().footer_mut().quota = quota;
    }
    let agents = crate::repl::live::modal::agents::refresh_agent_modals(controller, engine.agent_monitor());
    let should_redraw = expired || resized || quota_changed || agents;
    if !batch.ui.is_empty() || should_redraw {
        batch.flush(controller, should_redraw)?;
    }
    Ok(())
}

async fn handle_tick<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    batch: &mut LiveBatch,
    tick: IdleTick,
    ctx: &mut LiveIdleContext<'_, '_>,
) -> Result<()> {
    match tick {
        IdleTick::Frame => handle_frame_tick(controller, batch, ctx.session, ctx.engine)?,
        IdleTick::Quota => {
            sync_idle_quota(controller, batch, ctx.engine)?;
        }
        IdleTick::Ui(event) => {
            handle_ui_event(controller, batch, event).await?;
        }
    }
    Ok(())
}

async fn handle_input_source<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    event: Option<std::io::Result<Event>>,
    batch: &mut LiveBatch,
    resources: &mut EditorResources<'_>,
    input: &mut super::TerminalInputReader,
    ctx: &mut LiveIdleContext<'_, '_>,
) -> Result<IdleInputResult> {
    let Some(event) = event else {
        batch.flush(controller, false)?;
        return Err(anyhow::anyhow!("Terminal input reader stopped").into());
    };
    let event = event?;
    process_raw_input(controller, event, batch, resources, input, ctx).await
}

enum IdleStepOutcome {
    Continue,
    Message(QueuedMessage),
    Exit,
}

fn map_input_result(res: IdleInputResult) -> IdleStepOutcome {
    match res {
        IdleInputResult::Message(message) => IdleStepOutcome::Message(message),
        IdleInputResult::Exit => IdleStepOutcome::Exit,
        IdleInputResult::None => IdleStepOutcome::Continue,
    }
}

/// The follow-up that hands the model this session's finished background jobs, or `None` when the
/// wake-up was for another session or a turn already delivered the results. A wake-up for no known
/// session (missed wake-ups) checks this session's queue.
fn background_follow_up(session_id: &str, woken_for: Option<&str>) -> Option<String> {
    if woken_for.is_some_and(|owner| owner != session_id) {
        return None;
    }
    let notices = crate::engine::runner::turn::types::drain_background_notifications(session_id);
    (!notices.is_empty()).then(|| notices.join("\n\n"))
}

fn handle_bg_notification(ctx: &mut LiveIdleContext<'_, '_>, woken_for: Option<&str>) -> Result<IdleStepOutcome> {
    let Some(text) = background_follow_up(&ctx.engine.session_manager.session_id, woken_for) else {
        return Ok(IdleStepOutcome::Continue);
    };
    ctx.session.renderer.print_notice("\n● Background job completed\n");
    Ok(IdleStepOutcome::Message(QueuedMessage {
        text,
        kind: crate::ui::interactive::QueueKind::FollowUp,
    }))
}

async fn handle_idle_step_source<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    batch: &mut LiveBatch,
    resources: &mut EditorResources<'_>,
    input: &mut super::TerminalInputReader,
    ctx: &mut LiveIdleContext<'_, '_>,
    source: IdleSource,
) -> Result<IdleStepOutcome> {
    match source {
        IdleSource::Tick(tick) => {
            handle_tick(controller, batch, tick, ctx).await?;
            Ok(IdleStepOutcome::Continue)
        }
        IdleSource::Input(event) => {
            let res = handle_input_source(controller, event, batch, resources, input, ctx).await?;
            Ok(map_input_result(res))
        }
        IdleSource::BackgroundNotification(woken_for) => handle_bg_notification(ctx, woken_for.as_deref()),
        IdleSource::McpProblems => {
            crate::repl::report_late_mcp_problems(ctx.session, ctx.engine);
            Ok(IdleStepOutcome::Continue)
        }
    }
}

async fn drive_idle_loop<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    ui_events: &mut UiEventReceiver,
    input: &mut super::TerminalInputReader,
    resources: &mut EditorResources<'_>,
    ctx: &mut LiveIdleContext<'_, '_>,
) -> Result<Option<QueuedMessage>> {
    let mut ui = IdleUi::new(ctx.engine);
    crate::repl::report_late_mcp_problems(ctx.session, ctx.engine);
    loop {
        if ui.quota_rx.has_changed().is_err() {
            ui.quota_rx = ctx.engine.quota_subscribe();
        }
        let step = next_idle_step(&mut ui, input, ui_events).await;
        match handle_idle_step_source(controller, &mut ui.batch, resources, input, ctx, step).await? {
            IdleStepOutcome::Message(msg) => return Ok(Some(msg)),
            IdleStepOutcome::Exit => return Ok(None),
            IdleStepOutcome::Continue => {}
        }
    }
}

pub(crate) async fn read_idle_input<B: TerminalBackend>(ctx: IdleContext<'_, '_, B>) -> Result<Option<QueuedMessage>> {
    let LiveIo {
        controller,
        events: ui_events,
        input,
    } = ctx.io;
    let mut last_escape_time: Option<std::time::Instant> = None;
    let mut resources = EditorResources {
        history: ctx.editor.history,
        completions: ctx.editor.completions,
    };
    let mut idle_ctx = LiveIdleContext {
        session: ctx.session,
        engine: ctx.engine,
        last_escape_time: &mut last_escape_time,
    };
    drive_idle_loop(controller, ui_events, input, &mut resources, &mut idle_ctx).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::AuthStore;
    use crate::config::Config;
    use crate::engine::builder::AgentEngineBuilder;
    use crate::ui::interactive::InteractiveState;
    use crate::ui::interactive::UiEvent;
    use crate::ui::interactive::controller::tests::fake::FakeTerminal;

    async fn test_harness() -> (
        TerminalController<FakeTerminal>,
        crate::ui::interactive::controller::tests::fake::SharedWidth,
        LiveBatch,
        crate::repl::ReplSession,
        crate::engine::AgentEngine,
        tempfile::TempDir,
    ) {
        let temp = tempfile::tempdir().unwrap();
        let config = Config {
            provider: "antigravity".to_string(),
            model: "gemini-2.5-pro".to_string(),
            sessions_dir: temp.path().join("sessions"),
            ..Default::default()
        };
        let auth_store = AuthStore::default();
        let engine = AgentEngineBuilder::new(config.clone(), auth_store.clone())
            .build()
            .await
            .unwrap();
        let session = crate::repl::ReplSession::new(config, auth_store, None);
        let (backend, _, shared_width) = FakeTerminal::new(80);
        let controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
        let batch = LiveBatch::new();
        (controller, shared_width, batch, session, engine, temp)
    }

    #[tokio::test]
    async fn an_idle_prompt_hands_the_model_only_its_own_sessions_finished_jobs_once() {
        use crate::engine::runner::turn::types::{drain_background_notifications, enqueue_background_notification};
        let (_controller, _width, _batch, mut session, mut engine, _temp) = test_harness().await;
        let own = engine.session_manager.session_id.clone();
        enqueue_background_notification(&own, "first job done".into());
        enqueue_background_notification(&own, "second job done".into());
        enqueue_background_notification("idle-other-session", "foreign result".into());
        let mut last_escape_time = None;
        let mut ctx = LiveIdleContext {
            session: &mut session,
            engine: &mut engine,
            last_escape_time: &mut last_escape_time,
        };

        assert!(matches!(
            handle_bg_notification(&mut ctx, Some("idle-other-session")).unwrap(),
            IdleStepOutcome::Continue
        ));
        let IdleStepOutcome::Message(message) = handle_bg_notification(&mut ctx, Some(&own)).unwrap() else {
            panic!("expected a follow-up message");
        };
        assert_eq!(message.text, "first job done\n\nsecond job done");
        assert!(
            matches!(
                handle_bg_notification(&mut ctx, Some(&own)).unwrap(),
                IdleStepOutcome::Continue
            ),
            "results already handed over are not sent again"
        );
        enqueue_background_notification(&own, "third job done".into());
        let IdleStepOutcome::Message(missed) = handle_bg_notification(&mut ctx, None).unwrap() else {
            panic!("missed wake-ups still deliver this session's notices");
        };
        assert_eq!(missed.text, "third job done");
        assert_eq!(drain_background_notifications("idle-other-session"), ["foreign result"]);
    }

    #[tokio::test]
    async fn an_overflowed_notification_channel_wakes_the_idle_prompt_instead_of_going_quiet() {
        let (tx, mut rx) = tokio::sync::broadcast::channel(1);
        tx.send("a".to_string()).unwrap();
        tx.send("b".to_string()).unwrap();
        assert_eq!(
            wait_bg_notif(&mut rx).await,
            Some(None),
            "overflow is a wake-up for no known session"
        );
        assert_eq!(wait_bg_notif(&mut rx).await, Some(Some("b".to_string())));
        drop(tx);
        assert_eq!(wait_bg_notif(&mut rx).await, None);
    }

    #[tokio::test]
    async fn test_handle_tick_frame_and_quota() {
        let (mut controller, shared_width, mut batch, mut session, mut engine, _temp) = test_harness().await;
        let mut last_escape_time = None;
        let mut ctx = LiveIdleContext {
            session: &mut session,
            engine: &mut engine,
            last_escape_time: &mut last_escape_time,
        };

        // Frame tick with resize
        shared_width.set(100);
        handle_tick(&mut controller, &mut batch, IdleTick::Frame, &mut ctx)
            .await
            .unwrap();
        assert_eq!(controller.width(), 100);

        let ag_key = rho_engine::engine::tracking::QuotaKey::new("antigravity", Some("gemini-2.5-pro"));
        ctx.engine.quota().record_success(&ag_key, "75%".to_string());
        handle_tick(&mut controller, &mut batch, IdleTick::Quota, &mut ctx)
            .await
            .unwrap();
        assert_eq!(controller.state().footer().quota, Some("75%".to_string()));

        ctx.engine.quota().record_success(&ag_key, "50%".to_string());
        handle_tick(&mut controller, &mut batch, IdleTick::Frame, &mut ctx)
            .await
            .unwrap();
        assert_eq!(controller.state().footer().quota, Some("50%".to_string()));
    }

    #[tokio::test]
    async fn test_handle_tick_ui_event() {
        let (mut controller, _shared_width, mut batch, mut session, mut engine, _temp) = test_harness().await;
        let mut last_escape_time = None;
        let mut ctx = LiveIdleContext {
            session: &mut session,
            engine: &mut engine,
            last_escape_time: &mut last_escape_time,
        };

        handle_tick(
            &mut controller,
            &mut batch,
            IdleTick::Ui(UiEvent::RunningTool(Some("bash".to_string()))),
            &mut ctx,
        )
        .await
        .unwrap();

        assert!(!batch.ui.is_empty());

        handle_tick(&mut controller, &mut batch, IdleTick::Frame, &mut ctx)
            .await
            .unwrap();
        assert!(batch.ui.is_empty());
    }

    #[tokio::test]
    async fn test_wait_bg_notif_channels() {
        let (tx, mut rx) = tokio::sync::broadcast::channel(2);
        tx.send("notif".to_string()).unwrap();
        assert_eq!(wait_bg_notif(&mut rx).await, Some(Some("notif".to_string())));
        drop(tx);
        assert_eq!(wait_bg_notif(&mut rx).await, None);
    }

    #[test]
    fn test_idle_tick_from_frame_or_ui() {
        assert!(matches!(IdleTick::from(FrameOrUi::Frame), IdleTick::Frame));
        assert!(matches!(
            IdleTick::from(FrameOrUi::Ui(UiEvent::ToolEnd)),
            IdleTick::Ui(UiEvent::ToolEnd)
        ));
    }
}
