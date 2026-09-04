use crossterm::event::{KeyCode, KeyEvent};

use super::super::{ModalKeyResult, apply_input_edit};
use super::PendingModal;
use crate::error::Result;
use crate::ui::interactive::{InputAction, InteractionResponse, TerminalBackend, TerminalController, map_key};

pub(super) fn handle_input_mode_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
    pending: &mut Option<PendingModal>,
) -> Result<ModalKeyResult> {
    match key.code {
        KeyCode::Esc => {
            let has_options = controller.state().active_modal().is_some_and(|m| !m.options.is_empty());
            if has_options {
                if let Some(modal) = controller.state_mut().active_modal_mut() {
                    modal.exit_input_mode();
                }
            } else {
                controller.state_mut().pop_modal();
                if let Some(pending) = pending.take() {
                    let _ = pending.responder.respond(InteractionResponse::Cancelled);
                }
            }
        }
        KeyCode::Enter => {
            let custom = controller
                .state()
                .active_modal()
                .map(|m| m.input.text().trim().to_string())
                .unwrap_or_default();
            let input_option = controller.state().active_modal().and_then(|m| m.input_option);
            controller.state_mut().pop_modal();
            if let Some(pending) = pending.take() {
                let response = if let Some(index) = input_option {
                    InteractionResponse::SelectedWithInput { index, text: custom }
                } else if !custom.is_empty() {
                    InteractionResponse::Custom(custom)
                } else {
                    InteractionResponse::Cancelled
                };
                let _ = pending.responder.respond(response);
            }
        }
        _ => {
            if let InputAction::Edit(action) = map_key(key)
                && let Some(modal) = controller.state_mut().active_modal_mut()
            {
                apply_input_edit(&mut modal.input, action);
            }
        }
    }
    controller.redraw()?;
    Ok(ModalKeyResult::Handled)
}
