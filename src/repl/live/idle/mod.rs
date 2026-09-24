mod dispatch;
pub(crate) mod modal_action;
pub(crate) mod shortcut;

use dispatch::{IdleInputResult, process_raw_input};

use super::batch::{LiveBatch, OUTPUT_FRAME_INTERVAL};
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
}

impl IdleUi {
    fn new(engine: &crate::engine::AgentEngine) -> Self {
        let mut frame = tokio::time::interval(OUTPUT_FRAME_INTERVAL);
        frame.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        Self {
            batch: LiveBatch::new(),
            frame,
            quota_rx: engine.quota_subscribe(),
        }
    }
}

enum IdleTick {
    Frame,
    Quota,
    Ui(crate::ui::interactive::UiEvent),
}

enum IdleSource {
    Tick(IdleTick),
    Input(Option<std::io::Result<Event>>),
}

async fn wait_quota(rx: &mut tokio::sync::watch::Receiver<u64>) {
    if rx.changed().await.is_err() {
        std::future::pending::<()>().await;
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
        _ = wait_quota(&mut ui.quota_rx) => IdleSource::Tick(IdleTick::Quota),
        _ = ui.frame.tick() => IdleSource::Tick(IdleTick::Frame),
        Some(event) = ui_events.recv() => IdleSource::Tick(IdleTick::Ui(event)),
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

async fn handle_tick<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    batch: &mut LiveBatch,
    tick: IdleTick,
    ctx: &mut LiveIdleContext<'_, '_>,
) -> Result<()> {
    match tick {
        IdleTick::Frame => {
            let expired = controller.check_system_message_expiration();
            let resized = controller.refresh_size()?;
            if resized {
                ctx.session.renderer.set_width(controller.width());
            }
            let quota = ctx.engine.quota_display();
            let quota_changed = controller.state().footer().quota != quota;
            if quota_changed {
                controller.state_mut().footer_mut().quota = quota;
            }
            if !batch.ui.is_empty() || expired || resized || quota_changed {
                batch.flush(controller, expired || resized || quota_changed)?;
            }
        }
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

async fn drive_idle_loop<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    ui_events: &mut UiEventReceiver,
    input: &mut super::TerminalInputReader,
    resources: &mut EditorResources<'_>,
    ctx: &mut LiveIdleContext<'_, '_>,
) -> Result<Option<QueuedMessage>> {
    let mut ui = IdleUi::new(ctx.engine);
    loop {
        if ui.quota_rx.has_changed().is_err() {
            ui.quota_rx = ctx.engine.quota_subscribe();
        }
        match next_idle_step(&mut ui, input, ui_events).await {
            IdleSource::Tick(tick) => {
                handle_tick(controller, &mut ui.batch, tick, ctx).await?;
            }
            IdleSource::Input(event) => {
                match handle_input_source(controller, event, &mut ui.batch, resources, input, ctx).await? {
                    IdleInputResult::Message(message) => return Ok(Some(message)),
                    IdleInputResult::Exit => return Ok(None),
                    IdleInputResult::None => {}
                }
            }
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
