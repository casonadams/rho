mod input_mode;
mod prompt;
mod select_mode;

#[cfg(test)]
pub use prompt::is_input_trigger;
pub(crate) use prompt::prompt_label_for;

use crossterm::event::KeyEvent;

use input_mode::handle_input_mode_key;
use select_mode::handle_select_mode_key;

use super::ModalKeyResult;
use crate::error::Result;
use crate::ui::interactive::{
    InteractionPrompt, InteractionResponder, ModalMode, ModalOption, ModalState, TerminalBackend, TerminalController,
    UiEvent,
};

pub struct PendingModal {
    pub(crate) responder: InteractionResponder,
}

fn build_interaction_state(prompt: InteractionPrompt) -> ModalState {
    let options = prompt
        .options
        .into_iter()
        .map(|o| ModalOption {
            label: o.label,
            description: o.description,
            input: o.input,
        })
        .collect::<Vec<_>>();
    let is_empty = options.is_empty();
    let mut state = ModalState::new(prompt.title, prompt.body, options)
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
    let UiEvent::Interaction { prompt, responder } = event else {
        unreachable!("only interaction events create ordered barriers");
    };
    controller.state_mut().push_modal(build_interaction_state(prompt));
    *modal = Some(PendingModal { responder });
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
