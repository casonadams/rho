use crossterm::event::{KeyCode, KeyEvent};

use super::super::ModalKeyResult;
use super::PendingModal;
use super::prompt::{is_input_trigger, prompt_label_for};
use crate::error::Result;
use crate::ui::interactive::{
    InputAction, InteractionResponse, TerminalBackend, TerminalController, UiAction, map_key,
};

pub(super) fn handle_select_mode_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
    pending: &mut Option<PendingModal>,
) -> Result<ModalKeyResult> {
    match key.code {
        KeyCode::Up | KeyCode::BackTab => controller.state_mut().select_previous_modal_option(),
        KeyCode::Down | KeyCode::Tab => controller.state_mut().select_next_modal_option(),
        KeyCode::Char('j') | KeyCode::Char('k') => {
            let captures_typing = controller
                .state()
                .active_modal()
                .is_some_and(|m| m.allow_custom || m.is_searchable);
            if !captures_typing {
                if key.code == KeyCode::Char('j') {
                    controller.state_mut().select_next_modal_option();
                } else {
                    controller.state_mut().select_previous_modal_option();
                }
            }
        }
        KeyCode::Esc => {
            controller.state_mut().pop_modal();
            if let Some(pending) = pending.take() {
                let _ = pending.responder.respond(InteractionResponse::Cancelled);
            }
        }
        KeyCode::Enter => handle_select_enter(controller, pending),
        _ => {
            if let InputAction::Edit(UiAction::Insert(c)) = map_key(key) {
                let allow_custom = controller.state().active_modal().is_some_and(|m| m.allow_custom);
                if allow_custom && let Some(modal) = controller.state_mut().active_modal_mut() {
                    let prompt = prompt_label_for(&modal.title);
                    modal.enter_input_mode(prompt);
                    modal.input.insert(c);
                }
            }
        }
    }
    controller.redraw()?;
    Ok(ModalKeyResult::Handled)
}

fn handle_select_enter<B: TerminalBackend>(controller: &mut TerminalController<B>, pending: &mut Option<PendingModal>) {
    let selected = controller.state().active_modal().map_or(0, |modal| modal.selected);
    let selected_label = controller
        .state()
        .active_modal()
        .and_then(|m| m.selected_option())
        .map(|opt| opt.label.clone())
        .unwrap_or_default();

    let option_input = controller
        .state()
        .active_modal()
        .and_then(|m| m.options.get(selected))
        .and_then(|opt| opt.input.clone());

    if let Some(spec) = option_input {
        if let Some(modal) = controller.state_mut().active_modal_mut() {
            modal.selected = selected;
            modal.input_option = Some(selected);
            modal.enter_input_mode(&spec.label);
            if let Some(prefill) = spec.value {
                modal.input.set_text(prefill);
            }
        }
    } else if is_input_trigger(&selected_label) {
        let prompt = prompt_label_for(&selected_label);
        if let Some(modal) = controller.state_mut().active_modal_mut() {
            modal.enter_input_mode(prompt);
        }
    } else {
        controller.state_mut().pop_modal();
        if let Some(pending) = pending.take() {
            let _ = pending.responder.respond(InteractionResponse::Selected(selected));
        }
    }
}
