mod dispatch;
mod editor;
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
type KeyRest<'a, 'b, 'c> = (
    &'a mut ReplSession,
    &'b mut crate::engine::AgentEngine,
    &'c mut Option<std::time::Instant>,
);

struct IdleUi {
    batch: LiveBatch,
    frame: tokio::time::Interval,
}

impl IdleUi {
    fn new() -> Self {
        let mut frame = tokio::time::interval(OUTPUT_FRAME_INTERVAL);
        frame.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        Self {
            batch: LiveBatch::new(),
            frame,
        }
    }
}

enum IdleTick {
    Frame,
    Ui(Option<crate::ui::interactive::UiEvent>),
}

async fn next_frame_or_ui(frame: &mut tokio::time::Interval, ui: &mut UiEventReceiver) -> IdleTick {
    tokio::select! {
        biased;
        _ = frame.tick() => IdleTick::Frame,
        event = ui.recv() => IdleTick::Ui(event),
    }
}

enum IdleSource {
    Tick(IdleTick),
    Input(Option<std::io::Result<Event>>),
}

async fn next_idle_step(
    frame: &mut tokio::time::Interval,
    input: &mut super::TerminalInputReader,
    ui: &mut UiEventReceiver,
) -> IdleSource {
    tokio::select! {
        biased;
        tick = next_frame_or_ui(frame, ui) => IdleSource::Tick(tick),
        event = input.recv() => IdleSource::Input(event),
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
    (batch, tick): (&mut LiveBatch, IdleTick),
) -> Result<()> {
    match tick {
        IdleTick::Frame => {
            let expired = controller.check_system_message_expiration();
            batch.flush(controller, expired)?;
        }
        IdleTick::Ui(event) => {
            if let Some(event) = event {
                handle_ui_event(controller, batch, event).await?;
            }
        }
    }
    Ok(())
}

async fn handle_input_source<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    (event, batch, resources, input, rest): (
        Option<std::io::Result<Event>>,
        &mut LiveBatch,
        &mut EditorResources<'_>,
        &mut super::TerminalInputReader,
        &mut KeyRest<'_, '_, '_>,
    ),
) -> Result<IdleInputResult> {
    let Some(event) = event else {
        batch.flush(controller, false)?;
        return Err(anyhow::anyhow!("Terminal input reader stopped").into());
    };
    let event = event?;
    process_raw_input(controller, (event, batch, resources, input, rest)).await
}

async fn drive_idle_loop<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    (ui_events, input): (&mut UiEventReceiver, &mut super::TerminalInputReader),
    (resources, rest): (&mut EditorResources<'_>, &mut KeyRest<'_, '_, '_>),
) -> Result<Option<QueuedMessage>> {
    let mut ui = IdleUi::new();
    loop {
        match next_idle_step(&mut ui.frame, input, ui_events).await {
            IdleSource::Tick(tick) => handle_tick(controller, (&mut ui.batch, tick)).await?,
            IdleSource::Input(event) => {
                let args = (event, &mut ui.batch, &mut *resources, &mut *input, &mut *rest);
                match handle_input_source(controller, args).await? {
                    IdleInputResult::Message(msg) => return Ok(Some(msg)),
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
    let mut rest: KeyRest = (&mut *ctx.session, &mut *ctx.engine, &mut last_escape_time);
    drive_idle_loop(controller, (ui_events, input), (&mut resources, &mut rest)).await
}
