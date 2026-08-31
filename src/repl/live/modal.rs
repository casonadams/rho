use crate::error::Result;
use crate::ui::interactive::{
    InputAction, InteractionResponder, InteractionResponse, ModalState, TerminalController, UiAction, UiEvent, map_key,
};
use crossterm::event::KeyCode;

pub struct PendingModal {
    pub(crate) responder: InteractionResponder,
}

pub fn install_interaction(
    controller: &mut TerminalController<crate::ui::interactive::CrosstermBackend>,
    event: UiEvent,
    modal: &mut Option<PendingModal>,
) {
    let UiEvent::Interaction { prompt, responder } = event else {
        unreachable!("only interaction events create ordered barriers");
    };
    let options = prompt
        .options
        .into_iter()
        .map(|option| crate::ui::interactive::ModalOption {
            label: option.label,
            description: option.description,
        })
        .collect::<Vec<_>>();
    let is_empty_options = options.is_empty();
    let mut state = ModalState::new(prompt.title, prompt.body, options).with_custom(prompt.allow_custom);
    state.selected = prompt.initial_selection.min(state.options.len().saturating_sub(1));
    if is_empty_options || (prompt.allow_custom && state.options.is_empty()) {
        state.enter_input_mode("answer");
    }
    controller.state_mut().push_modal(state);
    *modal = Some(PendingModal { responder });
}

pub fn handle_modal_key(
    controller: &mut TerminalController<crate::ui::interactive::CrosstermBackend>,
    key: crossterm::event::KeyEvent,
    pending: &mut Option<PendingModal>,
) -> Result<bool> {
    let Some(active) = controller.state().active_modal() else {
        return Ok(false);
    };

    match &active.mode {
        crate::ui::interactive::ModalMode::Input { .. } => match key.code {
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
                controller.state_mut().pop_modal();
                if let Some(pending) = pending.take() {
                    let response = if !custom.is_empty() {
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
                    match action {
                        UiAction::Insert(c) => modal.input.insert(c),
                        UiAction::Backspace => modal.input.backspace(),
                        UiAction::Delete => modal.input.delete(),
                        UiAction::MoveLeft => modal.input.move_left(),
                        UiAction::MoveRight => modal.input.move_right(),
                        UiAction::MoveToStart => modal.input.move_to_start(),
                        UiAction::MoveToEnd => modal.input.move_to_end(),
                        _ => {}
                    }
                }
            }
        },
        crate::ui::interactive::ModalMode::Select => match key.code {
            KeyCode::Up | KeyCode::BackTab => controller.state_mut().select_previous_modal_option(),
            KeyCode::Down | KeyCode::Tab => controller.state_mut().select_next_modal_option(),
            KeyCode::Esc => {
                controller.state_mut().pop_modal();
                if let Some(pending) = pending.take() {
                    let _ = pending.responder.respond(InteractionResponse::Cancelled);
                }
            }
            KeyCode::Enter => {
                let selected = controller.state().active_modal().map_or(0, |modal| modal.selected);
                let selected_label = controller
                    .state()
                    .active_modal()
                    .and_then(|m| m.selected_option())
                    .map(|opt| opt.label.clone())
                    .unwrap_or_default();

                let triggers_input = selected_label.contains("with reason")
                    || selected_label.contains("with feedback")
                    || selected_label.contains("custom answer")
                    || selected_label.contains("custom input")
                    || selected_label.contains("Type something")
                    || selected_label.contains("Type a custom")
                    || selected_label == "Deny with reason";

                if triggers_input {
                    let prompt_label = if selected_label.contains("reason") || selected_label.contains("feedback") {
                        "reason"
                    } else {
                        "answer"
                    };
                    if let Some(modal) = controller.state_mut().active_modal_mut() {
                        modal.enter_input_mode(prompt_label);
                    }
                } else {
                    controller.state_mut().pop_modal();
                    if let Some(pending) = pending.take() {
                        let _ = pending.responder.respond(InteractionResponse::Selected(selected));
                    }
                }
            }
            _ => {
                if let InputAction::Edit(UiAction::Insert(c)) = map_key(key) {
                    let allow_custom = controller.state().active_modal().is_some_and(|m| m.allow_custom);
                    if allow_custom && let Some(modal) = controller.state_mut().active_modal_mut() {
                        let prompt_label = if modal.title.contains("Permission") || modal.title.contains("Approve") {
                            "reason"
                        } else {
                            "answer"
                        };
                        modal.enter_input_mode(prompt_label);
                        modal.input.insert(c);
                    }
                }
            }
        },
    }
    controller.redraw()?;
    Ok(true)
}
