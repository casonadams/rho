use crate::error::Result;
use crate::ui::interactive::{
    InputAction, InteractionResponder, InteractionResponse, ModalMode, ModalOption, ModalState, TerminalBackend,
    TerminalController, UiAction, UiEvent, map_key,
};
use crossterm::event::{KeyCode, KeyEvent};

use super::{ModalKeyResult, apply_input_edit};

pub struct PendingModal {
    pub(crate) responder: InteractionResponder,
}

pub fn install_interaction<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    event: UiEvent,
    modal: &mut Option<PendingModal>,
) {
    let UiEvent::Interaction { prompt, responder } = event else {
        unreachable!("only interaction events create ordered barriers");
    };
    let options = prompt
        .options
        .into_iter()
        .map(|option| ModalOption {
            label: option.label,
            description: option.description,
            input: option.input,
        })
        .collect::<Vec<_>>();
    let is_empty_options = options.is_empty();
    let mut state = ModalState::new(prompt.title, prompt.body, options).with_custom(prompt.allow_custom);
    state.selected = prompt.initial_selection.min(state.options.len().saturating_sub(1));
    if is_empty_options || (prompt.allow_custom && state.options.is_empty()) {
        state.enter_input_mode("input");
    }
    if let Some(prefill) = prompt.initial_text {
        state.enter_input_mode("input");
        state.input.set_text(prefill);
    }
    controller.state_mut().push_modal(state);
    *modal = Some(PendingModal { responder });
}

pub(crate) fn is_input_trigger(label: &str) -> bool {
    const PATTERNS: &[&str] = &[
        "with reason",
        "with feedback",
        "custom answer",
        "custom input",
        "Type something",
        "Type a custom",
        "Deny with reason",
        "Accept input",
    ];
    PATTERNS.iter().any(|p| label.contains(p))
}

pub(crate) fn prompt_label_for(label: &str) -> &'static str {
    if label.contains("reason")
        || label.contains("feedback")
        || label.contains("Permission")
        || label.contains("Approve")
    {
        "reason"
    } else {
        "answer"
    }
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

fn handle_input_mode_key<B: TerminalBackend>(
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

fn handle_select_mode_key<B: TerminalBackend>(
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
