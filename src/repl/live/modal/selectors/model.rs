use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::super::ModalKeyResult;
use crate::error::Result;
use crate::repl::ReplSession;
use crate::ui::interactive::{ModalOption, ModalState, TerminalBackend, TerminalController};
use rho_harness_core::config::ModelSlot;

pub const MODEL_SELECTOR_TITLE: &str = "Select Model";

fn is_default_model(session: &ReplSession, canonical_id: &str) -> bool {
    session.config.default_model.as_deref() == Some(canonical_id)
}

fn build_model_option(session: &ReplSession, item: &crate::repl::interactive::ModelItem) -> ModalOption {
    let canonical = item.canonical_id();
    let active_spec = session.config.canonical_model_spec();
    let is_active = canonical == active_spec || item.id == session.config.model;
    let active_mark = if is_active { "✓" } else { "" };
    let default_mark = if is_default_model(session, &canonical) {
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
    let discovered = session.listed_models();
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

fn slot_selector_options(session: &ReplSession, slot: ModelSlot) -> (Vec<ModalOption>, usize) {
    let current_model = session.config.slot_model(slot);
    let none_mark = if current_model.is_none() { "✓" } else { "" };
    let mut options = vec![ModalOption::new(
        "None",
        Some(format!(
            "none\t{none_mark}\t\tDisable {} model (fallback to default)",
            slot.title()
        )),
    )];
    let mut selected = 0;
    for (i, item) in session.listed_models().iter().enumerate() {
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

fn slot_selector_title(slot: ModelSlot) -> String {
    format!("Select {} Model", slot.title())
}

/// The slot whose model selector carries `title`.
pub fn slot_of_title(title: &str) -> Option<ModelSlot> {
    ModelSlot::ALL
        .into_iter()
        .find(|&slot| slot_selector_title(slot) == title)
}

pub fn open_slot_model_selector<B: TerminalBackend>(
    session: &ReplSession,
    controller: &mut TerminalController<B>,
    slot: ModelSlot,
) {
    let (options, selected) = slot_selector_options(session, slot);
    let mut modal = ModalState::new(slot_selector_title(slot), catalog_status(session), options).with_search(true);
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
    } else if let Some(slot) = slot_of_title(&title) {
        slot_selector_options(session, slot).0
    } else {
        return;
    };
    let body = catalog_status(session);
    if let Some(modal) = controller.state_mut().active_modal_mut() {
        modal.body = body;
        modal.replace_options(options);
    }
}

fn extract_selected_model<B: TerminalBackend>(controller: &TerminalController<B>) -> Option<(String, String)> {
    let opt = controller.state().active_modal().and_then(|m| m.selected_option())?;
    let label = opt.label.clone();
    if label == "None" {
        return Some(("None".to_string(), "none".to_string()));
    }
    let described = opt.description.as_deref().and_then(|d| d.split('\t').next());
    let route = rho_harness_core::provider::ModelRoute::parse(&label, None, described.unwrap_or("anthropic"));
    Some((label, route.connection.to_string()))
}

/// Closes the selector and reports its highlighted model through `selected`.
fn pop_and_select<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    selected: impl FnOnce(String, String) -> ModalKeyResult,
) -> Result<ModalKeyResult> {
    let choice = extract_selected_model(controller);
    controller.state_mut().pop_modal();
    controller.redraw()?;
    Ok(choice.map_or(ModalKeyResult::Handled, |(model, provider)| selected(model, provider)))
}

fn handle_selector_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
    select: impl FnOnce(&mut TerminalController<B>) -> Result<ModalKeyResult>,
) -> Result<ModalKeyResult> {
    match key.code {
        KeyCode::Enter => select(controller),
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

pub fn handle_model_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
) -> Result<ModalKeyResult> {
    let ctrl_s = key.code == KeyCode::Char('s') && key.modifiers.contains(KeyModifiers::CONTROL);
    let save_as_default = ctrl_s || controller.state().active_modal().is_some_and(|m| m.save_as_default);
    let select = |controller: &mut TerminalController<B>| {
        pop_and_select(controller, |model, provider| ModalKeyResult::ModelSelected {
            model,
            provider,
            save_as_default,
        })
    };
    if ctrl_s {
        return select(controller);
    }
    handle_selector_key(controller, key, select)
}

pub fn handle_slot_model_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
    slot: ModelSlot,
) -> Result<ModalKeyResult> {
    handle_selector_key(controller, key, |controller| {
        pop_and_select(controller, |model, provider| ModalKeyResult::SlotModelSelected {
            slot,
            model,
            provider,
        })
    })
}
