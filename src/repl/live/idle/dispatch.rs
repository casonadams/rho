use super::editor::open_external_editor;
use super::modal_action::{ModalActionContext, apply_modal_key_result};
use super::shortcut::{IdleShortcutContext, handle_shortcut_action};
use super::{EditorResources, KeyRest};
use crate::error::Result;
use crate::repl::interactive::{CompletionSet, InteractiveHistory};
use crate::repl::live::autocomplete::{AutocompleteKeyResult, handle_autocomplete_key, update_autocomplete_state};
use crate::repl::live::batch::LiveBatch;
use crate::repl::live::modal::{handle_modal_key, handle_modal_paste};
use crate::repl::live::navigation::{apply_completion, navigate_history_next, navigate_history_previous};
use crate::ui::interactive::{
    InputAction, QueuedMessage, TerminalBackend, TerminalController, UiAction, UiEffect, map_key,
};
use crossterm::event::{Event, KeyEvent};

pub(super) enum RawInput {
    Resize,
    Paste(String),
    Focus(bool),
    Key(KeyEvent),
    Skip,
}

pub(super) fn classify_event(event: Event) -> RawInput {
    match event {
        Event::Resize(_, _) => RawInput::Resize,
        Event::Paste(text) => RawInput::Paste(text),
        Event::FocusGained => RawInput::Focus(true),
        Event::FocusLost => RawInput::Focus(false),
        Event::Key(key) if key.kind != crossterm::event::KeyEventKind::Release => RawInput::Key(key),
        _ => RawInput::Skip,
    }
}

pub(super) fn handle_paste<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    (batch, text, completions): (&mut LiveBatch, String, &CompletionSet),
) -> Result<()> {
    if !handle_modal_paste(controller, &text) {
        controller.state_mut().apply(UiAction::Paste(text));
        update_autocomplete_state(controller, completions);
    }
    batch.flush(controller, true)
}

fn handle_history_nav<B: TerminalBackend>(
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

fn handle_edit_action<B: TerminalBackend>(
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

fn handle_dequeue<B: TerminalBackend>(controller: &mut TerminalController<B>, batch: &mut LiveBatch) -> Result<()> {
    let queued = controller.state_mut().dequeue_all();
    if !queued.is_empty() {
        let text = queued.into_iter().map(|m| m.text).collect::<Vec<_>>().join("\n");
        controller.state_mut().editor_mut().set_text(&text);
        batch.flush(controller, true)?;
    }
    Ok(())
}

async fn handle_misc_action<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    (action, batch, resources, input): (
        &InputAction,
        &mut LiveBatch,
        &mut EditorResources<'_>,
        &mut crate::repl::input_reader::TerminalInputReader,
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

pub(super) enum IdleInputResult {
    Message(QueuedMessage),
    Exit,
    None,
}

async fn handle_plain_action<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    (action, batch, resources, input): (
        &InputAction,
        &mut LiveBatch,
        &mut EditorResources<'_>,
        &mut crate::repl::input_reader::TerminalInputReader,
    ),
) -> Result<IdleInputResult> {
    match action {
        InputAction::Edit(edit) => {
            let res = handle_edit_action(controller, (batch, edit.clone(), resources.completions))?;
            Ok(res.map_or(IdleInputResult::None, IdleInputResult::Message))
        }
        InputAction::HistoryPrevious | InputAction::HistoryNext => {
            handle_history_nav(
                controller,
                (matches!(action, InputAction::HistoryNext), batch, resources.history),
            )?;
            Ok(IdleInputResult::None)
        }
        InputAction::Complete | InputAction::ExternalEditor | InputAction::DequeueQueued => {
            handle_misc_action(controller, (action, batch, resources, input)).await?;
            Ok(IdleInputResult::None)
        }
        InputAction::EndOfInput if controller.state().editor().is_empty() => {
            batch.flush(controller, false)?;
            Ok(IdleInputResult::Exit)
        }
        _ => Ok(IdleInputResult::None),
    }
}

async fn handle_plain_or_shortcut<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    (action, batch, resources, input, rest): (
        &InputAction,
        &mut LiveBatch,
        &mut EditorResources<'_>,
        &mut crate::repl::input_reader::TerminalInputReader,
        &mut KeyRest<'_, '_, '_>,
    ),
) -> Result<IdleInputResult> {
    match handle_plain_action(controller, (action, batch, resources, input)).await? {
        IdleInputResult::None => {}
        other => return Ok(other),
    }
    let (session, engine, last_escape_time) = rest;
    if !matches!(action, InputAction::EndOfInput | InputAction::Ignore) {
        let is_display_toggle = matches!(action, InputAction::ToggleExpandTools | InputAction::ThinkingToggle);
        handle_shortcut_action(
            action.clone(),
            IdleShortcutContext {
                controller,
                session,
                engine,
                last_escape_time,
            },
            batch,
        )
        .await?;
        if !is_display_toggle {
            batch.flush(controller, true)?;
        }
    }
    Ok(IdleInputResult::None)
}

enum KeyPhase {
    Handled,
    FallThrough,
}

async fn try_modal_key<B: TerminalBackend>(
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

async fn process_key_event<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    (key, batch, resources, input, rest): (
        KeyEvent,
        &mut LiveBatch,
        &mut EditorResources<'_>,
        &mut crate::repl::input_reader::TerminalInputReader,
        &mut KeyRest<'_, '_, '_>,
    ),
) -> Result<IdleInputResult> {
    if let KeyPhase::Handled = try_modal_key(controller, (key, batch, resources, &mut *rest)).await? {
        return Ok(IdleInputResult::None);
    }
    let action = map_key(key);
    handle_plain_or_shortcut(controller, (&action, batch, resources, input, &mut *rest)).await
}

pub(super) async fn process_raw_input<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    (event, batch, resources, input, rest): (
        Event,
        &mut LiveBatch,
        &mut EditorResources<'_>,
        &mut crate::repl::input_reader::TerminalInputReader,
        &mut KeyRest<'_, '_, '_>,
    ),
) -> Result<IdleInputResult> {
    match classify_event(event) {
        RawInput::Resize => {
            controller.refresh_size()?;
            Ok(IdleInputResult::None)
        }
        RawInput::Paste(text) => {
            handle_paste(controller, (batch, text, resources.completions)).map(|_| IdleInputResult::None)
        }
        RawInput::Focus(focused) => {
            if controller.focused() != focused {
                controller.set_focused(focused);
                batch.flush(controller, true)?;
            }
            Ok(IdleInputResult::None)
        }
        RawInput::Key(key) => process_key_event(controller, (key, batch, resources, input, rest)).await,
        RawInput::Skip => Ok(IdleInputResult::None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_event_focus_events() {
        assert!(matches!(classify_event(Event::FocusGained), RawInput::Focus(true)));
        assert!(matches!(classify_event(Event::FocusLost), RawInput::Focus(false)));
    }
}
