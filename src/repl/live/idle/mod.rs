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

async fn next_idle_step(
    ui: &mut IdleUi,
    input: &mut super::TerminalInputReader,
    ui_events: &mut UiEventReceiver,
) -> IdleSource {
    tokio::select! {
        biased;
        event = input.recv() => IdleSource::Input(event),
        res = ui.quota_rx.changed() => {
            if res.is_ok() {
                IdleSource::Tick(IdleTick::Quota)
            } else {
                tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
                IdleSource::Tick(IdleTick::Quota)
            }
        }
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
        controller.state_mut().footer_mut().quota = quota.clone();
        if crate::platform::remote::is_remote_active() {
            let totals = engine.session_usage_totals();
            crate::platform::remote::PEER_REGISTRY.broadcast(&rho_harness_core::rpc::protocol::RpcEvent::UsageUpdate {
                input_tokens: Some(totals.total_input),
                output_tokens: Some(totals.total_output),
                cache_read_tokens: Some(totals.total_cache_read),
                cache_write_tokens: Some(totals.total_cache_write),
                total_cost: None,
                context_percent: engine.context_percent_f64(),
                context_window: engine.context_limit(),
                tokens_per_second: engine.tokens_per_second(),
                quota,
            });
        }
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
        match next_idle_step(&mut ui, input, ui_events).await {
            IdleSource::Tick(tick) => {
                handle_tick(controller, &mut ui.batch, tick, ctx).await?;
                if let Some(prompt) = crate::platform::remote::REMOTE_PROMPT_QUEUE.pop() {
                    return Ok(Some(QueuedMessage {
                        text: prompt,
                        kind: crate::ui::interactive::QueueKind::FollowUp,
                    }));
                }
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
