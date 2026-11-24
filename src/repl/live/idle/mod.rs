pub(crate) mod dispatch;
pub(crate) mod modal_action;
pub(crate) mod shortcut;

use dispatch::{IdleInputResult, process_raw_input};

use super::batch::{LiveBatch, OUTPUT_FRAME_INTERVAL};
use super::types::{FrameOrUi, next_frame_or_ui, wait_version_change};
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
    catalog_rx: tokio::sync::watch::Receiver<u64>,
    pr_rx: tokio::sync::watch::Receiver<u64>,
    mcp: Option<std::sync::Arc<rho_engine::mcp::McpRuntime>>,
}

impl IdleUi {
    fn new(engine: &crate::engine::AgentEngine, session: &ReplSession) -> Self {
        let mut frame = tokio::time::interval(OUTPUT_FRAME_INTERVAL);
        frame.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        Self {
            batch: LiveBatch::new().with_monitor(engine.agent_monitor().clone()),
            frame,
            quota_rx: engine.quota_subscribe(),
            catalog_rx: session.catalog.subscribe(),
            pr_rx: crate::ui::interactive::footer::subscribe_git_pr(),
            mcp: engine.mcp_runtime().cloned(),
        }
    }
}

enum IdleTick {
    Frame,
    Quota,
    Catalog,
    Pr,
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
    Mail,
    McpProblems,
}

async fn wait_mcp_problems(runtime: Option<&rho_engine::mcp::McpRuntime>) {
    match runtime {
        Some(runtime) => runtime.problems_ready().await,
        None => std::future::pending().await,
    }
}

async fn next_idle_step(
    ui: &mut IdleUi,
    inbox: &rho_engine::mailbox::Mailbox,
    input: &mut super::TerminalInputReader,
    ui_events: &mut UiEventReceiver,
) -> IdleSource {
    tokio::select! {
        biased;
        event = input.recv() => IdleSource::Input(event),
        () = inbox.wait() => IdleSource::Mail,
        () = wait_mcp_problems(ui.mcp.as_deref()) => IdleSource::McpProblems,
        _ = wait_version_change(&mut ui.quota_rx) => IdleSource::Tick(IdleTick::Quota),
        _ = wait_version_change(&mut ui.catalog_rx) => IdleSource::Tick(IdleTick::Catalog),
        _ = wait_version_change(&mut ui.pr_rx) => IdleSource::Tick(IdleTick::Pr),
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

pub(crate) fn sync_idle_pr<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    batch: &mut LiveBatch,
) -> Result<bool> {
    let footer = controller.state().footer();
    if footer.status_bar_config.is_none() {
        return Ok(false);
    }
    let Some(cwd) = footer.cwd.as_deref() else {
        return Ok(false);
    };
    let pr = crate::ui::interactive::footer::cached_git_pr(std::path::Path::new(cwd), footer.git_branch.as_deref());
    if footer.git_pr != pr {
        controller.state_mut().footer_mut().git_pr = pr;
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
    let agents = crate::repl::live::modal::agents::refresh_agent_modals(controller, engine.agent_monitor())?;
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
        IdleTick::Catalog => {
            crate::repl::live::modal::refresh_open_model_selector(ctx.session, controller);
            batch.flush(controller, true)?;
        }
        IdleTick::Pr => {
            sync_idle_pr(controller, batch)?;
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

/// The follow-up that hands the agent the messages waiting in its mailbox (finished background work,
/// messages from its subagents), or `None` when a turn already delivered them.
fn mail_follow_up(inbox: &rho_engine::mailbox::Mailbox) -> Option<String> {
    let parent = inbox.parent();
    let messages: Vec<String> = inbox.take().iter().map(|m| m.for_model(parent.as_deref())).collect();
    (!messages.is_empty()).then(|| messages.join("\n\n"))
}

fn handle_mail(ctx: &mut LiveIdleContext<'_, '_>) -> Result<IdleStepOutcome> {
    let Some(text) = mail_follow_up(&ctx.engine.inbox()) else {
        return Ok(IdleStepOutcome::Continue);
    };
    ctx.session.renderer.print_notice("\n● Background work reported\n");
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
        IdleSource::Mail => handle_mail(ctx),
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
    let mut ui = IdleUi::new(ctx.engine, ctx.session);
    crate::repl::report_late_mcp_problems(ctx.session, ctx.engine);
    loop {
        if ui.quota_rx.has_changed().is_err() {
            ui.quota_rx = ctx.engine.quota_subscribe();
        }
        let inbox = ctx.engine.inbox();
        let step = next_idle_step(&mut ui, &inbox, input, ui_events).await;
        drop(inbox);
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
        use rho_engine::mailbox::{Envelope, Kind, Party};
        let (_controller, _width, _batch, mut session, mut engine, _temp) = test_harness().await;
        let inbox = engine.inbox();
        let other = inbox.registry().open("idle-other-session", None);
        for (to, body) in [
            (inbox.id(), "first job done"),
            (inbox.id(), "second job done"),
            (other.id(), "foreign result"),
        ] {
            inbox
                .registry()
                .send(Envelope::new(Party::System, to, Kind::Event, body))
                .unwrap();
        }
        let mut last_escape_time = None;
        let mut ctx = LiveIdleContext {
            session: &mut session,
            engine: &mut engine,
            last_escape_time: &mut last_escape_time,
        };

        let IdleStepOutcome::Message(message) = handle_mail(&mut ctx).unwrap() else {
            panic!("expected a follow-up message");
        };
        assert_eq!(message.text, "first job done\n\nsecond job done");
        assert!(
            matches!(handle_mail(&mut ctx).unwrap(), IdleStepOutcome::Continue),
            "results already handed over are not sent again"
        );
        assert_eq!(other.take().len(), 1, "another agent's mail stays with it");
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

    #[test]
    fn test_idle_tick_from_frame_or_ui() {
        assert!(matches!(IdleTick::from(FrameOrUi::Frame), IdleTick::Frame));
        assert!(matches!(
            IdleTick::from(FrameOrUi::Ui(UiEvent::ToolEnd)),
            IdleTick::Ui(UiEvent::ToolEnd)
        ));
    }
}
