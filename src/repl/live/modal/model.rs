use crate::error::Result;
use crate::repl::ReplSession;
use crate::ui::interactive::{ModalOption, ModalState, TerminalBackend, TerminalController};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::ModalKeyResult;

fn is_default_model(session: &ReplSession, model_id: &str, provider: &str) -> bool {
    session.config.default_model.as_deref().is_some_and(|dm| {
        model_id == dm
            && session
                .config
                .default_provider
                .as_deref()
                .is_none_or(|dp| provider == dp)
    })
}

fn build_model_option(session: &ReplSession, item: &crate::repl::interactive::ModelItem) -> ModalOption {
    let active_mark = if item.id == session.config.model { "✓" } else { "" };
    let default_mark = if is_default_model(session, &item.id, &item.provider) {
        "default"
    } else {
        ""
    };
    ModalOption::new(
        item.id.clone(),
        Some(format!(
            "{}\t{}\t{}\t{}",
            item.provider, active_mark, default_mark, item.description
        )),
    )
}

pub fn open_model_selector<B: TerminalBackend>(session: &ReplSession, controller: &mut TerminalController<B>) {
    let discovered = crate::repl::interactive::discover_models(&session.config, &session.auth_store);
    let mut options = Vec::new();
    let mut initial_selection = 0;

    for (i, item) in discovered.iter().enumerate() {
        if item.id == session.config.model {
            initial_selection = i;
        }
        options.push(build_model_option(session, item));
    }

    let mut modal = ModalState::new("Select Model", "", options).with_search(true);
    modal.selected = initial_selection;
    controller.state_mut().push_modal(modal);
}

fn extract_selected_model<B: TerminalBackend>(controller: &TerminalController<B>) -> Option<(String, String)> {
    let opt = controller.state().active_modal().and_then(|m| m.selected_option())?;
    let selected_model = opt.label.clone();
    let provider = opt
        .description
        .as_deref()
        .and_then(|d| d.split('\t').next())
        .unwrap_or("anthropic")
        .to_string();
    Some((selected_model, provider))
}

fn pop_and_select<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    save_as_default: bool,
) -> Result<ModalKeyResult> {
    let selected = extract_selected_model(controller);
    controller.state_mut().pop_modal();
    controller.redraw()?;
    Ok(match selected {
        Some((model, provider)) => ModalKeyResult::ModelSelected {
            model,
            provider,
            save_as_default,
        },
        None => ModalKeyResult::Handled,
    })
}

fn apply_model_filter<B: TerminalBackend>(controller: &mut TerminalController<B>, character: Option<char>) {
    if let Some(modal) = controller.state_mut().active_modal_mut() {
        let mut query = modal.filter_query.clone();
        if let Some(c) = character {
            query.push(c);
        } else {
            query.pop();
        }
        modal.set_filter(&query);
    }
    controller.redraw().ok();
}

fn is_plain_char(key: &KeyEvent) -> bool {
    !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
}

fn clear_filter_action<B: TerminalBackend>(controller: &mut TerminalController<B>) -> bool {
    let has_filter = controller
        .state()
        .active_modal()
        .is_some_and(|m| !m.filter_query.is_empty());
    if has_filter {
        if let Some(modal) = controller.state_mut().active_modal_mut() {
            modal.set_filter("");
        }
        controller.redraw().ok();
        return true;
    }
    controller.state_mut().pop_modal();
    controller.redraw().ok();
    false
}

fn handle_model_nav<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: &KeyEvent,
) -> Result<ModalKeyResult> {
    match key.code {
        KeyCode::Up | KeyCode::BackTab => {
            controller.state_mut().select_previous_modal_option();
            controller.redraw()?;
        }
        KeyCode::Tab if key.modifiers.contains(KeyModifiers::SHIFT) => {
            controller.state_mut().select_previous_modal_option();
            controller.redraw()?;
        }
        KeyCode::Down | KeyCode::Tab => {
            controller.state_mut().select_next_modal_option();
            controller.redraw()?;
        }
        KeyCode::Backspace => apply_model_filter(controller, None),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            let _ = clear_filter_action(controller);
        }
        KeyCode::Char(c) if is_plain_char(key) => apply_model_filter(controller, Some(c)),
        _ => {}
    }
    Ok(ModalKeyResult::Handled)
}

pub fn handle_model_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
) -> Result<ModalKeyResult> {
    if key.code == KeyCode::Char('s') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return pop_and_select(controller, true);
    }
    match key.code {
        KeyCode::Enter => pop_and_select(controller, false),
        KeyCode::Esc => {
            controller.state_mut().pop_modal();
            controller.redraw()?;
            Ok(ModalKeyResult::Handled)
        }
        _ => handle_model_nav(controller, &key),
    }
}
