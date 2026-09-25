use super::EditorResources;
use super::LiveIdleContext;
use super::modal_action::{ModalActionContext, apply_modal_key_result};
use super::shortcut::{IdleShortcutContext, handle_shortcut_action};
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

pub(crate) enum RawInput {
    Resize(u16, u16),
    Paste(String),
    Focus(bool),
    Key(KeyEvent),
    Skip,
}

pub(crate) fn classify_event(event: Event) -> RawInput {
    match event {
        Event::Resize(cols, rows) => RawInput::Resize(cols, rows),
        Event::Paste(text) => RawInput::Paste(text),
        Event::FocusGained => RawInput::Focus(true),
        Event::FocusLost => RawInput::Focus(false),
        Event::Key(key) if key.kind != crossterm::event::KeyEventKind::Release => RawInput::Key(key),
        _ => RawInput::Skip,
    }
}

pub(super) fn handle_paste<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    batch: &mut LiveBatch,
    text: String,
    completions: &CompletionSet,
) -> Result<()> {
    if !handle_modal_paste(controller, &text) {
        controller.state_mut().apply(UiAction::Paste(text));
        update_autocomplete_state(controller, completions);
    }
    batch.flush(controller, true)
}

fn handle_history_nav<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    next: bool,
    batch: &mut LiveBatch,
    history: &mut InteractiveHistory,
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
    batch: &mut LiveBatch,
    action: UiAction,
    completions: &CompletionSet,
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

fn handle_completion_action<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    batch: &mut LiveBatch,
    completions: &CompletionSet,
) -> Result<()> {
    if apply_completion(controller, completions) {
        batch.flush(controller, true)?;
    }
    Ok(())
}

pub(crate) async fn handle_misc_action<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    action: &InputAction,
    batch: &mut LiveBatch,
    resources: &mut EditorResources<'_>,
    input: &mut crate::repl::input_reader::TerminalInputReader,
) -> Result<()> {
    match action {
        InputAction::Complete => handle_completion_action(controller, batch, resources.completions),
        InputAction::ExternalEditor => {
            open_external_editor(controller, input).await?;
            batch.flush(controller, true)
        }
        _ => handle_dequeue(controller, batch),
    }
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

fn resume_terminal<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    paused: crate::repl::input_reader::PausedInput<'_>,
) -> Result<()> {
    let controller_res = controller.resume();
    let input_res = paused.resume();
    controller_res?;
    input_res?;
    Ok(())
}

async fn open_external_editor<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    input: &mut crate::repl::input_reader::TerminalInputReader,
) -> Result<()> {
    let current_text = controller.state().editor().text().to_string();
    let temp_file = std::env::temp_dir().join(format!("rho_draft_{}.md", uuid::Uuid::new_v4()));
    let _ = tokio::fs::write(&temp_file, &current_text).await;
    let editor = resolve_editor_command();
    let paused = input.pause()?;
    controller.suspend()?;
    let _status = tokio::process::Command::new(&editor).arg(&temp_file).status().await;
    resume_terminal(controller, paused)?;
    apply_edited_text(controller, &temp_file).await;
    let _ = tokio::fs::remove_file(temp_file).await;
    Ok(())
}

pub(crate) enum IdleInputResult {
    None,
    Message(QueuedMessage),
    Exit,
}

enum PlainActionResult {
    Message(QueuedMessage),
    Exit,
    Handled,
    Unhandled,
}

async fn handle_plain_action<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    action: &InputAction,
    batch: &mut LiveBatch,
    resources: &mut EditorResources<'_>,
    input: &mut crate::repl::input_reader::TerminalInputReader,
) -> Result<PlainActionResult> {
    match action {
        InputAction::Edit(edit) => {
            let res = handle_edit_action(controller, batch, edit.clone(), resources.completions)?;
            Ok(res.map_or(PlainActionResult::Handled, PlainActionResult::Message))
        }
        InputAction::HistoryPrevious | InputAction::HistoryNext => {
            handle_history_nav(
                controller,
                matches!(action, InputAction::HistoryNext),
                batch,
                resources.history,
            )?;
            Ok(PlainActionResult::Handled)
        }
        InputAction::Complete | InputAction::ExternalEditor | InputAction::DequeueQueued => {
            handle_misc_action(controller, action, batch, resources, input).await?;
            Ok(PlainActionResult::Handled)
        }
        InputAction::EndOfInput if controller.state().editor().is_empty() => {
            batch.flush(controller, false)?;
            Ok(PlainActionResult::Exit)
        }
        _ => Ok(PlainActionResult::Unhandled),
    }
}

async fn handle_plain_or_shortcut<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    action: &InputAction,
    batch: &mut LiveBatch,
    resources: &mut EditorResources<'_>,
    input: &mut crate::repl::input_reader::TerminalInputReader,
    ctx: &mut LiveIdleContext<'_, '_>,
) -> Result<IdleInputResult> {
    match handle_plain_action(controller, action, batch, resources, input).await? {
        PlainActionResult::Message(msg) => return Ok(IdleInputResult::Message(msg)),
        PlainActionResult::Exit => return Ok(IdleInputResult::Exit),
        PlainActionResult::Handled => return Ok(IdleInputResult::None),
        PlainActionResult::Unhandled => {}
    }
    if !matches!(action, InputAction::EndOfInput | InputAction::Ignore) {
        handle_shortcut_action(
            action.clone(),
            IdleShortcutContext {
                controller,
                session: ctx.session,
                engine: ctx.engine,
                last_escape_time: ctx.last_escape_time,
            },
            batch,
        )
        .await?;
    }
    Ok(IdleInputResult::None)
}

enum KeyPhase {
    Handled,
    FallThrough,
}

async fn try_modal_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
    batch: &mut LiveBatch,
    resources: &mut EditorResources<'_>,
    input: &mut crate::repl::input_reader::TerminalInputReader,
    ctx: &mut LiveIdleContext<'_, '_>,
) -> Result<KeyPhase> {
    let modal_res = handle_modal_key(controller, key, &mut batch.modal)?;
    let modal = ModalActionContext {
        controller,
        history: resources.history,
        session: ctx.session,
        engine: ctx.engine,
        input,
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
    key: KeyEvent,
    batch: &mut LiveBatch,
    resources: &mut EditorResources<'_>,
    input: &mut crate::repl::input_reader::TerminalInputReader,
    ctx: &mut LiveIdleContext<'_, '_>,
) -> Result<IdleInputResult> {
    if let KeyPhase::Handled = try_modal_key(controller, key, batch, resources, input, ctx).await? {
        return Ok(IdleInputResult::None);
    }
    let action = map_key(key);
    handle_plain_or_shortcut(controller, &action, batch, resources, input, ctx).await
}

fn sync_renderer_width(
    renderer: &crate::ui::TerminalRenderer,
    controller: &TerminalController<impl TerminalBackend>,
    resized: bool,
) {
    if resized {
        renderer.set_width(controller.width());
    }
}

pub(crate) fn handle_resize<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    renderer: &crate::ui::TerminalRenderer,
    batch: &mut LiveBatch,
    cols: u16,
    rows: u16,
) -> Result<()> {
    let resized = controller.resize_to(usize::from(cols), usize::from(rows))? || controller.refresh_size()?;
    sync_renderer_width(renderer, controller, resized);
    batch.flush(controller, true)
}

pub(crate) fn handle_focus<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    renderer: &crate::ui::TerminalRenderer,
    batch: &mut LiveBatch,
    focused: bool,
) -> Result<()> {
    let resized = controller.refresh_size()?;
    sync_renderer_width(renderer, controller, resized);
    if controller.focused() != focused || resized {
        controller.set_focused(focused);
        batch.flush(controller, true)?;
    }
    Ok(())
}

pub(crate) fn handle_raw_paste<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    renderer: &crate::ui::TerminalRenderer,
    batch: &mut LiveBatch,
    text: String,
    completions: &CompletionSet,
) -> Result<()> {
    if controller.refresh_size()? {
        renderer.set_width(controller.width());
        batch.flush(controller, true)?;
    }
    handle_paste(controller, batch, text, completions)
}

pub(crate) async fn process_raw_input<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    event: Event,
    batch: &mut LiveBatch,
    resources: &mut EditorResources<'_>,
    input: &mut crate::repl::input_reader::TerminalInputReader,
    ctx: &mut LiveIdleContext<'_, '_>,
) -> Result<IdleInputResult> {
    match classify_event(event) {
        RawInput::Resize(cols, rows) => {
            handle_resize(controller, &ctx.session.renderer, batch, cols, rows)?;
            Ok(IdleInputResult::None)
        }
        RawInput::Paste(text) => {
            handle_raw_paste(controller, &ctx.session.renderer, batch, text, resources.completions)?;
            Ok(IdleInputResult::None)
        }
        RawInput::Focus(focused) => {
            handle_focus(controller, &ctx.session.renderer, batch, focused)?;
            Ok(IdleInputResult::None)
        }
        RawInput::Key(key) => process_key_event(controller, key, batch, resources, input, ctx).await,
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

    #[test]
    fn test_classify_event_resize() {
        assert!(matches!(
            classify_event(Event::Resize(80, 24)),
            RawInput::Resize(80, 24)
        ));
    }
}
