use crate::error::Result;
use crate::ui::interactive::{
    InputAction, InteractionResponder, InteractionResponse, ModalState, TerminalController, UiAction, UiEvent, map_key,
};
use crossterm::event::{KeyCode, KeyModifiers};

pub struct PendingModal {
    pub(crate) responder: InteractionResponder,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModalKeyResult {
    NotHandled,
    Handled,
    ModelSelected {
        model: String,
        provider: String,
        save_as_default: bool,
    },
}

pub fn install_interaction<B: crate::ui::interactive::TerminalBackend>(
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

pub fn open_model_selector<B: crate::ui::interactive::TerminalBackend>(
    session: &crate::repl::ReplSession,
    controller: &mut TerminalController<B>,
) {
    let discovered = crate::repl::interactive::discover_models(&session.config, &session.auth_store);
    let mut options = Vec::new();
    let mut initial_selection = 0;

    for (i, item) in discovered.iter().enumerate() {
        let is_active = item.id == session.config.model;
        if is_active {
            initial_selection = i;
        }
        let active_tag = if is_active { " [ACTIVE]" } else { "" };
        options.push(crate::ui::interactive::ModalOption::new(
            item.id.clone(),
            Some(format!("{} · {}{active_tag}", item.provider, item.description)),
        ));
    }

    let mut modal = ModalState::new(
        "Select Model",
        "Select from configured and well-known AI models. Use /login to add provider credentials.",
        options,
    );
    modal.selected = initial_selection;
    controller.state_mut().push_modal(modal);
}

pub fn handle_modal_key<B: crate::ui::interactive::TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: crossterm::event::KeyEvent,
    pending: &mut Option<PendingModal>,
) -> Result<ModalKeyResult> {
    let Some(active) = controller.state().active_modal() else {
        return Ok(ModalKeyResult::NotHandled);
    };

    let is_model_selector = active.title == "Select Model";

    if is_model_selector {
        if key.code == KeyCode::Char('s') && key.modifiers.contains(KeyModifiers::CONTROL) {
            if let Some(opt) = controller.state().active_modal().and_then(|m| m.selected_option()) {
                let selected_model = opt.label.clone();
                let provider = opt
                    .description
                    .as_deref()
                    .and_then(|d| d.split_whitespace().next())
                    .unwrap_or("anthropic")
                    .to_string();
                controller.state_mut().pop_modal();
                return Ok(ModalKeyResult::ModelSelected {
                    model: selected_model,
                    provider,
                    save_as_default: true,
                });
            }
            controller.state_mut().pop_modal();
            return Ok(ModalKeyResult::Handled);
        }

        match key.code {
            KeyCode::Up | KeyCode::BackTab => {
                controller.state_mut().select_previous_modal_option();
                controller.redraw()?;
                return Ok(ModalKeyResult::Handled);
            }
            KeyCode::Down | KeyCode::Tab => {
                controller.state_mut().select_next_modal_option();
                controller.redraw()?;
                return Ok(ModalKeyResult::Handled);
            }
            KeyCode::Enter => {
                if let Some(opt) = controller.state().active_modal().and_then(|m| m.selected_option()) {
                    let selected_model = opt.label.clone();
                    let provider = opt
                        .description
                        .as_deref()
                        .and_then(|d| d.split_whitespace().next())
                        .unwrap_or("anthropic")
                        .to_string();
                    controller.state_mut().pop_modal();
                    return Ok(ModalKeyResult::ModelSelected {
                        model: selected_model,
                        provider,
                        save_as_default: false,
                    });
                }
                controller.state_mut().pop_modal();
                return Ok(ModalKeyResult::Handled);
            }
            KeyCode::Esc => {
                controller.state_mut().pop_modal();
                controller.redraw()?;
                return Ok(ModalKeyResult::Handled);
            }
            KeyCode::Backspace => {
                if let Some(modal) = controller.state_mut().active_modal_mut() {
                    let mut query = modal.filter_query.clone();
                    query.pop();
                    modal.set_filter(&query);
                }
                controller.redraw()?;
                return Ok(ModalKeyResult::Handled);
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                controller.state_mut().pop_modal();
                controller.redraw()?;
                return Ok(ModalKeyResult::Handled);
            }
            KeyCode::Char(c) if !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => {
                if let Some(modal) = controller.state_mut().active_modal_mut() {
                    let mut query = modal.filter_query.clone();
                    query.push(c);
                    modal.set_filter(&query);
                }
                controller.redraw()?;
                return Ok(ModalKeyResult::Handled);
            }
            _ => return Ok(ModalKeyResult::Handled),
        }
    }

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
    Ok(ModalKeyResult::Handled)
}
