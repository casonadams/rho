use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::super::{ModalKeyResult, handle_selector_key, select_highlighted};
use crate::error::Result;
use crate::repl::ReplSession;
use crate::ui::interactive::{ModalKind, ModalOption, ModalState, TerminalBackend, TerminalController};
use rho_harness_core::config::ModelRole;
use rho_harness_core::provider::ModelRoute;

fn is_default_model(session: &ReplSession, canonical_id: &str) -> bool {
    session.config.default_model.as_deref() == Some(canonical_id)
}

fn item_route(item: &crate::repl::interactive::ModelItem) -> ModelRoute {
    ModelRoute::new(&item.provider, &item.id)
}

/// The route an option's value carries, as written by `ModelRoute::to_string`.
fn route_from_value(value: &str) -> Option<ModelRoute> {
    value.contains('/').then(|| ModelRoute::parse(value, None, ""))
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
    .with_value(item_route(item).to_string())
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
    let mut modal = ModalState::new("Select Model", catalog_status(session), options)
        .with_search(true)
        .with_save_as_default(save_as_default)
        .with_kind(ModalKind::Model);
    modal.selected = selected;
    controller.state_mut().push_modal(modal);
}

fn role_selector_options(session: &ReplSession, role: &ModelRole) -> (Vec<ModalOption>, usize) {
    let current_model = session.config.role_model(role.as_str());
    let none_mark = if current_model.is_none() { "✓" } else { "" };
    let mut options = vec![ModalOption::new(
        "None",
        Some(format!(
            "none\t{none_mark}\t\tDisable {} model (fallback to default)",
            role.title()
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
        options.push(
            ModalOption::new(
                canonical,
                Some(format!("{}\t{}\t\t{}", item.provider, mark, item.description)),
            )
            .with_value(item_route(item).to_string()),
        );
    }
    (options, selected)
}

fn role_selector_title(role: &ModelRole) -> String {
    format!("Select {} Model", role.title())
}

pub fn open_role_model_selector<B: TerminalBackend>(
    session: &ReplSession,
    controller: &mut TerminalController<B>,
    role: &ModelRole,
) {
    let (options, selected) = role_selector_options(session, role);
    let mut modal = ModalState::new(role_selector_title(role), catalog_status(session), options)
        .with_search(true)
        .with_kind(ModalKind::RoleModel(role.clone()));
    modal.selected = selected;
    controller.state_mut().push_modal(modal);
}

/// Rebuilds an open model selector from the current catalog, keeping the search and selection.
pub fn refresh_open_model_selector<B: TerminalBackend>(session: &ReplSession, controller: &mut TerminalController<B>) {
    let Some(kind) = controller.state().active_modal().map(|m| m.kind.clone()) else {
        return;
    };
    let options = match kind {
        ModalKind::Model => model_selector_options(session).0,
        ModalKind::RoleModel(role) => role_selector_options(session, &role).0,
        _ => return,
    };
    let body = catalog_status(session);
    if let Some(modal) = controller.state_mut().active_modal_mut() {
        modal.body = body;
        modal.replace_options(options);
    }
}

/// Maps the highlighted model option to `ModelSelected`; `None` when it carries no route.
fn model_selection(opt: &ModalOption, save_as_default: bool) -> Option<ModalKeyResult> {
    let route = route_from_value(opt.value.as_deref()?)?;
    Some(ModalKeyResult::ModelSelected {
        model: route.to_string(),
        provider: route.connection.to_string(),
        save_as_default,
    })
}

/// `None` for the clear option (no route), `Some(route)` for a model; `None` when the value is malformed.
fn role_selection(opt: &ModalOption) -> Option<Option<ModelRoute>> {
    match opt.value.as_deref() {
        None => Some(None),
        Some(value) => route_from_value(value).map(Some),
    }
}

pub fn handle_model_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
) -> Result<ModalKeyResult> {
    let ctrl_s = key.code == KeyCode::Char('s') && key.modifiers.contains(KeyModifiers::CONTROL);
    let save_as_default = ctrl_s || controller.state().active_modal().is_some_and(|m| m.save_as_default);
    let on_enter = move |opt: &ModalOption| model_selection(opt, save_as_default);
    if ctrl_s {
        return select_highlighted(controller, on_enter);
    }
    handle_selector_key(controller, key, |controller| select_highlighted(controller, on_enter))
}

pub fn handle_role_model_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
    role: ModelRole,
) -> Result<ModalKeyResult> {
    handle_selector_key(controller, key, |controller| {
        select_highlighted(controller, move |opt| {
            role_selection(opt).map(|model| ModalKeyResult::RoleModelSelected { role, model })
        })
    })
}
