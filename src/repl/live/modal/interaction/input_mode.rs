use crossterm::event::{KeyCode, KeyEvent};

use super::super::{ModalKeyResult, apply_input_edit};
use super::types::{PendingModal, build_enter_response};
use crate::error::Result;
use crate::ui::interactive::{
    InputAction, InteractionResponse, TerminalBackend, TerminalController, UiAction, map_key,
};

pub(crate) fn exit_input_or_pop_modal<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    pending: &mut Option<PendingModal>,
) {
    let has_options = controller.state().active_modal().is_some_and(|m| !m.options.is_empty());
    if has_options {
        if let Some(modal) = controller.state_mut().active_modal_mut() {
            modal.exit_input_mode();
        }
        return;
    }
    controller.state_mut().pop_modal();
    if let Some(pending) = pending.take() {
        let _ = pending.responder.respond(InteractionResponse::Cancelled);
    }
}

pub(crate) fn submit_enter<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    pending: &mut Option<PendingModal>,
) {
    let custom = controller
        .state()
        .active_modal()
        .map(|m| m.input.expanded_text().trim().to_string())
        .unwrap_or_default();
    let input_option = controller.state().active_modal().and_then(|m| m.input_option);
    controller.state_mut().pop_modal();
    if let Some(pending) = pending.take() {
        let response = build_enter_response(custom, input_option);
        let _ = pending.responder.respond(response);
    }
}

pub(crate) fn apply_modal_edit<B: TerminalBackend>(controller: &mut TerminalController<B>, edit: UiAction) {
    if let Some(modal) = controller.state_mut().active_modal_mut() {
        apply_input_edit(&mut modal.input, edit);
    }
}

fn handle_vertical_move<B: TerminalBackend>(controller: &mut TerminalController<B>, down: bool) {
    let width = controller.terminal_width().saturating_sub(2).max(1);
    if let Some(modal) = controller.state_mut().active_modal_mut() {
        if down {
            modal.input.move_down(width);
        } else {
            modal.input.move_up(width);
        }
    }
}

fn handle_plain_key<B: TerminalBackend>(controller: &mut TerminalController<B>, key: KeyEvent) {
    match map_key(key) {
        InputAction::Clear => {
            if let Some(modal) = controller.state_mut().active_modal_mut() {
                modal.input.set_text("");
            }
        }
        InputAction::HistoryPrevious => handle_vertical_move(controller, false),
        InputAction::HistoryNext => handle_vertical_move(controller, true),
        InputAction::ClipboardPasteImage => {
            if let Some(text) = crate::platform::clipboard::get_text_or_image_path() {
                apply_modal_edit(controller, UiAction::Paste(text));
            }
        }
        InputAction::Edit(action) => apply_modal_edit(controller, action),
        _ => {}
    }
}

fn is_newline_key(key: &KeyEvent) -> bool {
    (key.code == KeyCode::Enter
        && key.modifiers.intersects(
            crossterm::event::KeyModifiers::SHIFT
                | crossterm::event::KeyModifiers::ALT
                | crossterm::event::KeyModifiers::CONTROL,
        ))
        || (key.code == KeyCode::Char('j') && key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL))
        || key.code == KeyCode::Char('\n')
}

pub(crate) fn handle_input_mode_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
    pending: &mut Option<PendingModal>,
) -> Result<ModalKeyResult> {
    if key.code == KeyCode::Esc {
        exit_input_or_pop_modal(controller, pending);
    } else if is_newline_key(&key) {
        apply_modal_edit(controller, UiAction::InsertNewline);
    } else if key.code == KeyCode::Enter {
        submit_enter(controller, pending);
    } else {
        handle_plain_key(controller, key);
    }
    controller.redraw()?;
    Ok(ModalKeyResult::Handled)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    #[test]
    fn test_is_newline_key() {
        let shift_enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT);
        let ctrl_enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL);
        let alt_enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT);
        let ctrl_j = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL);
        let raw_lf = KeyEvent::new(KeyCode::Char('\n'), KeyModifiers::NONE);
        let plain_enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);

        assert!(is_newline_key(&shift_enter));
        assert!(is_newline_key(&ctrl_enter));
        assert!(is_newline_key(&alt_enter));
        assert!(is_newline_key(&ctrl_j));
        assert!(is_newline_key(&raw_lf));
        assert!(!is_newline_key(&plain_enter));
    }
}
