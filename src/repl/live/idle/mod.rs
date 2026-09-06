mod editor;
pub(crate) mod modal_action;
pub(crate) mod shortcut;

use editor::open_external_editor;
use modal_action::{ModalActionContext, apply_modal_key_result};
use shortcut::{IdleShortcutContext, handle_shortcut_action};

use super::autocomplete::{AutocompleteKeyResult, handle_autocomplete_key, update_autocomplete_state};
use super::batch::{LiveBatch, OUTPUT_FRAME_INTERVAL};
use super::modal::handle_modal_key;
use super::navigation::{apply_completion, navigate_history_next, navigate_history_previous};
use super::{EditorResources, IdleContext, LiveIo};
use crate::error::Result;
use crate::repl::ReplSession;
use crate::repl::interactive::{CompletionSet, InteractiveHistory};
use crate::ui::interactive::{InputAction, QueuedMessage, TerminalController, UiAction, UiEffect, map_key};
use crossterm::event::{Event, KeyEvent};

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

enum RawInput {
    Resize,
    Paste(String),
    Key(KeyEvent),
    Skip,
}

fn classify_event(event: Event) -> RawInput {
    match event {
        Event::Resize(_, _) => RawInput::Resize,
        Event::Paste(text) => RawInput::Paste(text),
        Event::Key(key) if key.kind != crossterm::event::KeyEventKind::Release => RawInput::Key(key),
        _ => RawInput::Skip,
    }
}

fn handle_paste<B: crate::ui::interactive::TerminalBackend>(
    controller: &mut TerminalController<B>,
    (batch, text, completions): (&mut LiveBatch, String, &CompletionSet),
) -> Result<()> {
    controller.state_mut().apply(UiAction::Paste(text));
    update_autocomplete_state(controller, completions);
    batch.flush(controller, true)
}

fn handle_history_nav<B: crate::ui::interactive::TerminalBackend>(
    controller: &mut TerminalController<B>,
    (next, batch, history): (bool, &mut LiveBatch, &mut InteractiveHistory),
) -> Result<()> {
    let moved = if next {
        navigate_history_next(controller, history)
    } else {
        navigate_history_previous(controller, history)
    };
    if moved {
        batch.flush(controller, true)?;
    }
    Ok(())
}

fn handle_edit_action<B: crate::ui::interactive::TerminalBackend>(
    controller: &mut TerminalController<B>,
    (batch, action, completions): (&mut LiveBatch, UiAction, &CompletionSet),
) -> Result<Option<QueuedMessage>> {
    let effect = controller.state_mut().apply(action);
    update_autocomplete_state(controller, completions);
    if let UiEffect::Queued(message) = effect {
        controller.state_mut().pop_queued();
        batch.flush(controller, true)?;
        return Ok(Some(message));
    }
    batch.flush(controller, true)?;
    Ok(None)
}

fn handle_dequeue<B: crate::ui::interactive::TerminalBackend>(
    controller: &mut TerminalController<B>,
    batch: &mut LiveBatch,
) -> Result<()> {
    let queued = controller.state_mut().dequeue_all();
    if !queued.is_empty() {
        let text = queued.into_iter().map(|m| m.text).collect::<Vec<_>>().join("\n");
        controller.state_mut().editor_mut().set_text(&text);
        batch.flush(controller, true)?;
    }
    Ok(())
}

async fn handle_misc_action<B: crate::ui::interactive::TerminalBackend>(
    controller: &mut TerminalController<B>,
    (action, batch, resources, input): (
        &InputAction,
        &mut LiveBatch,
        &mut EditorResources<'_>,
        &mut super::TerminalInputReader,
    ),
) -> Result<()> {
    match action {
        InputAction::Complete => {
            if apply_completion(controller, resources.completions) {
                batch.flush(controller, true)?;
            }
        }
        InputAction::ExternalEditor => {
            open_external_editor(controller, input).await?;
            batch.flush(controller, true)?;
        }
        _ => handle_dequeue(controller, batch)?,
    }
    Ok(())
}

async fn handle_plain_action<B: crate::ui::interactive::TerminalBackend>(
    controller: &mut TerminalController<B>,
    (action, batch, resources, input): (
        &InputAction,
        &mut LiveBatch,
        &mut EditorResources<'_>,
        &mut super::TerminalInputReader,
    ),
) -> Result<Option<QueuedMessage>> {
    match action {
        InputAction::Edit(edit) => handle_edit_action(controller, (batch, edit.clone(), resources.completions)),
        InputAction::HistoryPrevious | InputAction::HistoryNext => {
            handle_history_nav(
                controller,
                (matches!(action, InputAction::HistoryNext), batch, resources.history),
            )?;
            Ok(None)
        }
        InputAction::Complete | InputAction::ExternalEditor | InputAction::DequeueQueued => {
            handle_misc_action(controller, (action, batch, resources, input)).await?;
            Ok(None)
        }
        InputAction::EndOfInput if controller.state().editor().is_empty() => {
            batch.flush(controller, false)?;
            Ok(None)
        }
        _ => Ok(None),
    }
}

async fn handle_plain_or_shortcut<B: crate::ui::interactive::TerminalBackend>(
    controller: &mut TerminalController<B>,
    (action, batch, resources, input, rest): (
        &InputAction,
        &mut LiveBatch,
        &mut EditorResources<'_>,
        &mut super::TerminalInputReader,
        &mut KeyRest<'_, '_, '_>,
    ),
) -> Result<Option<QueuedMessage>> {
    if let Some(msg) = handle_plain_action(controller, (action, batch, resources, input)).await? {
        return Ok(Some(msg));
    }
    let (session, engine, last_escape_time) = rest;
    match action {
        InputAction::EndOfInput | InputAction::Ignore => {}
        shortcut_action => {
            handle_shortcut_action(
                shortcut_action.clone(),
                IdleShortcutContext {
                    controller,
                    session,
                    engine,
                    last_escape_time,
                },
                batch,
            )
            .await?;
            batch.flush(controller, true)?;
        }
    }
    Ok(None)
}

enum KeyPhase {
    Handled,
    FallThrough,
}

async fn try_modal_key<B: crate::ui::interactive::TerminalBackend>(
    controller: &mut TerminalController<B>,
    (key, batch, resources, rest): (
        KeyEvent,
        &mut LiveBatch,
        &mut EditorResources<'_>,
        &mut KeyRest<'_, '_, '_>,
    ),
) -> Result<KeyPhase> {
    let modal_res = handle_modal_key(controller, key, &mut batch.modal)?;
    let modal = ModalActionContext {
        controller,
        history: resources.history,
        session: rest.0,
        engine: rest.1,
    };
    if apply_modal_key_result(modal_res, modal, batch).await? {
        batch.flush(controller, true)?;
        return Ok(KeyPhase::Handled);
    }
    if matches!(
        handle_autocomplete_key(controller, resources.completions, key),
        AutocompleteKeyResult::Handled
    ) {
        batch.flush(controller, true)?;
        return Ok(KeyPhase::Handled);
    }
    Ok(KeyPhase::FallThrough)
}

async fn process_key_event<B: crate::ui::interactive::TerminalBackend>(
    controller: &mut TerminalController<B>,
    (key, batch, resources, input, rest): (
        KeyEvent,
        &mut LiveBatch,
        &mut EditorResources<'_>,
        &mut super::TerminalInputReader,
        &mut KeyRest<'_, '_, '_>,
    ),
) -> Result<Option<QueuedMessage>> {
    if let KeyPhase::Handled = try_modal_key(controller, (key, batch, resources, &mut *rest)).await? {
        return Ok(None);
    }
    let action = map_key(key);
    handle_plain_or_shortcut(controller, (&action, batch, resources, input, &mut *rest)).await
}

async fn process_raw_input<B: crate::ui::interactive::TerminalBackend>(
    controller: &mut TerminalController<B>,
    (event, batch, resources, input, rest): (
        Event,
        &mut LiveBatch,
        &mut EditorResources<'_>,
        &mut super::TerminalInputReader,
        &mut KeyRest<'_, '_, '_>,
    ),
) -> Result<Option<QueuedMessage>> {
    match classify_event(event) {
        RawInput::Resize => {
            controller.refresh_size()?;
            Ok(None)
        }
        RawInput::Paste(text) => handle_paste(controller, (batch, text, resources.completions)).map(|_| None),
        RawInput::Key(key) => process_key_event(controller, (key, batch, resources, input, rest)).await,
        RawInput::Skip => Ok(None),
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

async fn handle_ui_event<B: crate::ui::interactive::TerminalBackend>(
    controller: &mut TerminalController<B>,
    batch: &mut LiveBatch,
    event: crate::ui::interactive::UiEvent,
) -> Result<()> {
    batch.enqueue(controller, event)
}

async fn handle_tick<B: crate::ui::interactive::TerminalBackend>(
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

async fn handle_input_source<B: crate::ui::interactive::TerminalBackend>(
    controller: &mut TerminalController<B>,
    (event, batch, resources, input, rest): (
        Option<std::io::Result<Event>>,
        &mut LiveBatch,
        &mut EditorResources<'_>,
        &mut super::TerminalInputReader,
        &mut KeyRest<'_, '_, '_>,
    ),
) -> Result<Option<QueuedMessage>> {
    let Some(event) = event else {
        batch.flush(controller, false)?;
        return Err(anyhow::anyhow!("Terminal input reader stopped").into());
    };
    let event = event?;
    process_raw_input(controller, (event, batch, resources, input, rest)).await
}

pub(crate) async fn read_idle_input<B: crate::ui::interactive::TerminalBackend>(
    ctx: IdleContext<'_, '_, B>,
) -> Result<Option<QueuedMessage>> {
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
    let mut ui = IdleUi::new();

    loop {
        match next_idle_step(&mut ui.frame, &mut *input, ui_events).await {
            IdleSource::Tick(tick) => handle_tick(controller, (&mut ui.batch, tick)).await?,
            IdleSource::Input(event) => {
                let args = (event, &mut ui.batch, &mut resources, &mut *input, &mut rest);
                if let Some(msg) = handle_input_source(controller, args).await? {
                    return Ok(Some(msg));
                }
            }
        }
    }
}
