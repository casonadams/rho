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
    (batch, tick, rest): (&mut LiveBatch, IdleTick, &mut KeyRest<'_, '_, '_>),
) -> Result<()> {
    match tick {
        IdleTick::Frame => {
            let expired = controller.check_system_message_expiration();
            let resized = controller.refresh_size()?;
            if resized {
                rest.0.renderer.set_width(controller.width());
            }
            if !batch.ui.is_empty() || expired || resized {
                batch.flush(controller, expired || resized)?;
            }
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
            IdleSource::Tick(tick) => {
                handle_tick(controller, (&mut ui.batch, tick, &mut *rest)).await?;
                if let Some(prompt) = crate::platform::remote::REMOTE_PROMPT_QUEUE.pop() {
                    return Ok(Some(QueuedMessage {
                        text: prompt,
                        kind: crate::ui::interactive::QueueKind::FollowUp,
                    }));
                }
            }
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

fn resolve_editor_command() -> String {
    std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "nano".to_string())
}

async fn apply_edited_text<B: TerminalBackend>(controller: &mut TerminalController<B>, temp_file: &std::path::Path) {
    if let Ok(edited_text) = tokio::fs::read_to_string(temp_file).await {
        controller.state_mut().editor_mut().set_text(edited_text.trim_end());
    }
}

pub(super) async fn open_external_editor<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    input: &mut super::TerminalInputReader,
) -> Result<()> {
    let current_text = controller.state().editor().text().to_string();
    let temp_file = std::env::temp_dir().join(format!("rho_draft_{}.md", uuid::Uuid::new_v4()));
    let _ = tokio::fs::write(&temp_file, &current_text).await;
    let editor = resolve_editor_command();
    let paused = input.pause()?;
    controller.suspend()?;
    let _status = tokio::process::Command::new(&editor).arg(&temp_file).status().await;
    let controller_res = controller.resume();
    let input_res = paused.resume();
    controller_res?;
    input_res?;
    apply_edited_text(controller, &temp_file).await;
    let _ = tokio::fs::remove_file(temp_file).await;
    Ok(())
}
