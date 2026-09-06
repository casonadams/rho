use crossterm::event::{KeyCode, KeyEvent};

use super::super::{ModalKeyResult, apply_input_edit};
use super::PendingModal;
use crate::error::Result;
use crate::ui::interactive::UiAction;
use crate::ui::interactive::{InputAction, InteractionResponse, TerminalBackend, TerminalController, map_key};

fn exit_input_or_pop_modal<B: TerminalBackend>(
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

fn build_enter_response(custom: String, input_option: Option<usize>) -> InteractionResponse {
    if let Some(index) = input_option {
        InteractionResponse::SelectedWithInput { index, text: custom }
    } else if !custom.is_empty() {
        InteractionResponse::Custom(custom)
    } else {
        InteractionResponse::Cancelled
    }
}

fn submit_enter<B: TerminalBackend>(controller: &mut TerminalController<B>, pending: &mut Option<PendingModal>) {
    let custom = controller
        .state()
        .active_modal()
        .map(|m| m.input.text().trim().to_string())
        .unwrap_or_default();
    let input_option = controller.state().active_modal().and_then(|m| m.input_option);
    controller.state_mut().pop_modal();
    if let Some(pending) = pending.take() {
        let response = build_enter_response(custom, input_option);
        let _ = pending.responder.respond(response);
    }
}

fn apply_modal_edit<B: TerminalBackend>(controller: &mut TerminalController<B>, edit: UiAction) {
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
        InputAction::Edit(action) => apply_modal_edit(controller, action),
        _ => {}
    }
}

fn is_newline_key(key: &KeyEvent) -> bool {
    (key.code == KeyCode::Enter
        && key
            .modifiers
            .intersects(crossterm::event::KeyModifiers::SHIFT | crossterm::event::KeyModifiers::ALT))
        || (key.code == KeyCode::Char('j') && key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL))
}

pub(super) fn handle_input_mode_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
    pending: &mut Option<PendingModal>,
) -> Result<ModalKeyResult> {
    if is_newline_key(&key) {
        handle_plain_key(controller, key);
    } else {
        match key.code {
            KeyCode::Esc => exit_input_or_pop_modal(controller, pending),
            KeyCode::Enter => submit_enter(controller, pending),
            _ => handle_plain_key(controller, key),
        }
    }
    controller.redraw()?;
    Ok(ModalKeyResult::Handled)
}
