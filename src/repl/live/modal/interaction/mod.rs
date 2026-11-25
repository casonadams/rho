mod highlight;
mod input_mode;
mod select_mode;
pub(crate) mod types;

use crossterm::event::KeyEvent;
use input_mode::handle_input_mode_key;
pub use types::PendingModal;
pub(super) use types::prompt_label_for;

use super::ModalKeyResult;
use crate::error::Result;
use crate::ui::interactive::{
    InteractionPrompt, ModalMode, ModalOption, ModalState, TerminalBackend, TerminalController, UiEvent,
};
use select_mode::handle_select_mode_key;

fn build_interaction_state(prompt: InteractionPrompt, theme: &crate::ui::theme::Theme) -> ModalState {
    let options = prompt
        .options
        .into_iter()
        .map(|o| ModalOption {
            label: o.label,
            description: o.description,
            input: o.input,
            value: None,
        })
        .collect::<Vec<_>>();
    let is_empty = options.is_empty();
    let body = highlight::style_permission_body(&prompt.title, prompt.body, theme);
    let mut state = ModalState::new(prompt.title, body, options)
        .with_custom(prompt.allow_custom)
        .with_option_layout(prompt.option_layout);
    state.selected = prompt.initial_selection.min(state.options.len().saturating_sub(1));
    if is_empty || (prompt.allow_custom && state.options.is_empty()) || prompt.initial_text.is_some() {
        state.enter_input_mode("input");
    }
    if let Some(prefill) = prompt.initial_text {
        state.input.set_text(prefill);
    }
    state
}

pub fn install_interaction<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    event: UiEvent,
    modal: &mut Option<PendingModal>,
) {
    match event {
        UiEvent::Interaction { prompt, responder } => {
            let state = build_interaction_state(prompt, controller.theme());
            controller.state_mut().push_modal(state);
            *modal = Some(PendingModal::Interaction(responder));
        }
        UiEvent::A2uiModalRequest { request, response_tx } => {
            if let Some(a2ui) = crate::ui::interactive::a2ui::A2uiModalState::from_request(request) {
                controller.state_mut().push_modal(ModalState::from_a2ui(a2ui));
                *modal = Some(PendingModal::A2ui(response_tx));
            } else {
                let _ = response_tx.send(rho_harness_core::a2ui::A2uiModalResult::Cancelled);
            }
        }
        _ => unreachable!("only interaction events create ordered barriers"),
    }
}

pub fn handle_a2ui_modal_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
    pending: &mut Option<PendingModal>,
) -> Result<ModalKeyResult> {
    let outcome = {
        let Some(modal) = controller.state_mut().active_modal_mut() else {
            return Ok(ModalKeyResult::NotHandled);
        };
        let Some(a2ui) = modal.a2ui.as_mut() else {
            return Ok(ModalKeyResult::NotHandled);
        };
        a2ui.handle_key(key)
    };
    if let crate::ui::interactive::a2ui::A2uiKeyOutcome::Completed(result) = outcome {
        controller.state_mut().pop_modal();
        if let Some(pending) = pending.take() {
            pending.respond_a2ui(result);
        }
    }
    controller.redraw()?;
    Ok(ModalKeyResult::Handled)
}

pub fn handle_a2ui_modal_paste<B: TerminalBackend>(controller: &mut TerminalController<B>, text: &str) -> bool {
    let Some(modal) = controller.state_mut().active_modal_mut() else {
        return false;
    };
    let Some(a2ui) = modal.a2ui.as_mut() else {
        return false;
    };
    a2ui.handle_paste(text)
}

pub fn handle_interaction_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
    pending: &mut Option<PendingModal>,
) -> Result<ModalKeyResult> {
    let Some(active) = controller.state().active_modal() else {
        return Ok(ModalKeyResult::NotHandled);
    };

    match &active.mode {
        ModalMode::Input { .. } => handle_input_mode_key(controller, key, pending),
        ModalMode::Select => handle_select_mode_key(controller, key, pending),
    }
}
