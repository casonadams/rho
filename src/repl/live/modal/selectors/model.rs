use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::super::ModalKeyResult;
use crate::error::Result;
use crate::repl::ReplSession;
use crate::ui::interactive::{ModalOption, ModalState, TerminalBackend, TerminalController};

pub const MODEL_SELECTOR_TITLE: &str = "Select Model";

fn is_default_model(session: &ReplSession, canonical_id: &str, model_id: &str, provider: &str) -> bool {
    session.config.default_model.as_deref().is_some_and(|dm| {
        dm == canonical_id
            || (model_id == dm
                && session
                    .config
                    .default_provider
                    .as_deref()
                    .is_none_or(|dp| provider == dp))
    })
}

fn build_model_option(session: &ReplSession, item: &crate::repl::interactive::ModelItem) -> ModalOption {
    let canonical = item.canonical_id();
    let active_spec = session.config.canonical_model_spec();
    let is_active = canonical == active_spec || item.id == session.config.model;
    let active_mark = if is_active { "✓" } else { "" };
    let default_mark = if is_default_model(session, &canonical, &item.id, &item.provider) {
        "default"
    } else {
        ""
    };
    ModalOption::new(
        canonical,
        Some(format!(
            "{}\t{}\t{}\t{}",
            item.provider, active_mark, default_mark, item.description
        )),
    )
}

pub fn open_model_selector<B: TerminalBackend>(session: &ReplSession, controller: &mut TerminalController<B>) {
    open_model_selector_with_default(session, controller, false);
}

fn model_selector_options(session: &ReplSession) -> (Vec<ModalOption>, usize) {
    let discovered = crate::repl::interactive::discover_models(&session.config, &session.catalog);
    let active_spec = session.config.canonical_model_spec();
    let selected = discovered
        .iter()
        .rposition(|item| item.canonical_id() == active_spec || item.id == session.config.model)
        .unwrap_or(0);
    let options = discovered
        .iter()
        .map(|item| build_model_option(session, item))
        .collect();
    (options, selected)
}

fn catalog_status(session: &ReplSession) -> String {
    crate::repl::interactive::catalog_status(&session.config, &session.catalog)
}

pub fn open_model_selector_with_default<B: TerminalBackend>(
    session: &ReplSession,
    controller: &mut TerminalController<B>,
    save_as_default: bool,
) {
    let (options, selected) = model_selector_options(session);
    let mut modal = ModalState::new(MODEL_SELECTOR_TITLE, catalog_status(session), options)
        .with_search(true)
        .with_save_as_default(save_as_default);
    modal.selected = selected;
    controller.state_mut().push_modal(modal);
}

fn role_selector_options(
    session: &ReplSession,
    role_name: &str,
    current_model: Option<&str>,
) -> (Vec<ModalOption>, usize) {
    let discovered = crate::repl::interactive::discover_models(&session.config, &session.catalog);
    let none_mark = if current_model.is_none() { "✓" } else { "" };
    let mut options = vec![ModalOption::new(
        "None",
        Some(format!(
            "none\t{none_mark}\t\tDisable {role_name} model (fallback to default)"
        )),
    )];
    let mut selected = 0;
    for (i, item) in discovered.iter().enumerate() {
        let canonical = item.canonical_id();
        let is_current = current_model.is_some_and(|m| m == canonical || m == item.id);
        if is_current {
            selected = i + 1;
        }
        let mark = if is_current { "✓" } else { "" };
        options.push(ModalOption::new(
            canonical,
            Some(format!("{}\t{}\t\t{}", item.provider, mark, item.description)),
        ));
    }
    (options, selected)
}

fn role_model(session: &ReplSession, role_name: &str) -> Option<String> {
    match role_name {
        "Guard" => session.config.guard_model().map(str::to_string),
        _ => session.config.models.get(&role_name.to_ascii_lowercase()).cloned(),
    }
}

pub fn open_role_model_selector<B: TerminalBackend>(
    session: &ReplSession,
    controller: &mut TerminalController<B>,
    role_name: &str,
    current_model: Option<&str>,
) {
    let (options, selected) = role_selector_options(session, role_name, current_model);
    let title = format!("Select {role_name} Model");
    let mut modal = ModalState::new(title, catalog_status(session), options).with_search(true);
    modal.selected = selected;
    controller.state_mut().push_modal(modal);
}

/// Rebuilds an open model selector from the current catalog, keeping the search and selection.
pub fn refresh_open_model_selector<B: TerminalBackend>(session: &ReplSession, controller: &mut TerminalController<B>) {
    let Some(title) = controller.state().active_modal().map(|m| m.title.clone()) else {
        return;
    };
    let options = if title == MODEL_SELECTOR_TITLE {
        model_selector_options(session).0
    } else if let Some(role) = title.strip_prefix("Select ").and_then(|t| t.strip_suffix(" Model")) {
        role_selector_options(session, role, role_model(session, role).as_deref()).0
    } else {
        return;
    };
    let body = catalog_status(session);
    if let Some(modal) = controller.state_mut().active_modal_mut() {
        modal.body = body;
        modal.replace_options(options);
    }
}

pub fn open_guard_model_selector<B: TerminalBackend>(session: &ReplSession, controller: &mut TerminalController<B>) {
    open_role_model_selector(session, controller, "Guard", session.config.guard_model());
}

pub fn open_smol_model_selector<B: TerminalBackend>(session: &ReplSession, controller: &mut TerminalController<B>) {
    let smol_model = session.config.models.get("smol").map(String::as_str);
    open_role_model_selector(session, controller, "Smol", smol_model);
}

pub fn open_slow_model_selector<B: TerminalBackend>(session: &ReplSession, controller: &mut TerminalController<B>) {
    let slow_model = session.config.models.get("slow").map(String::as_str);
    open_role_model_selector(session, controller, "Slow", slow_model);
}

fn extract_selected_model<B: TerminalBackend>(controller: &TerminalController<B>) -> Option<(String, String)> {
    let opt = controller.state().active_modal().and_then(|m| m.selected_option())?;
    let label = opt.label.clone();
    if label == "None" {
        return Some(("None".to_string(), "none".to_string()));
    }
    let described = opt.description.as_deref().and_then(|d| d.split('\t').next());
    let route = rho_harness_core::provider::ModelRoute::parse(&label, None, described.unwrap_or("anthropic"));
    Some((label, route.provider))
}

fn pop_and_select_model<B: TerminalBackend>(
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

pub fn handle_model_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
) -> Result<ModalKeyResult> {
    let save_as_default = controller.state().active_modal().is_some_and(|m| m.save_as_default);
    if key.code == KeyCode::Char('s') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return pop_and_select_model(controller, true);
    }
    match key.code {
        KeyCode::Enter => pop_and_select_model(controller, save_as_default),
        KeyCode::Esc => {
            crate::repl::live::modal::pop_and_cancel(controller)?;
            Ok(ModalKeyResult::Handled)
        }
        _ => {
            crate::repl::live::modal::handle_selector_nav(controller, &key)?;
            Ok(ModalKeyResult::Handled)
        }
    }
}

fn pop_and_select_guard_model<B: TerminalBackend>(controller: &mut TerminalController<B>) -> Result<ModalKeyResult> {
    let selected = extract_selected_model(controller);
    controller.state_mut().pop_modal();
    controller.redraw()?;
    Ok(match selected {
        Some((model, provider)) => ModalKeyResult::GuardModelSelected { model, provider },
        None => ModalKeyResult::Handled,
    })
}

pub fn handle_guard_model_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
) -> Result<ModalKeyResult> {
    match key.code {
        KeyCode::Enter => pop_and_select_guard_model(controller),
        KeyCode::Esc => {
            crate::repl::live::modal::pop_and_cancel(controller)?;
            Ok(ModalKeyResult::Handled)
        }
        _ => {
            crate::repl::live::modal::handle_selector_nav(controller, &key)?;
            Ok(ModalKeyResult::Handled)
        }
    }
}
