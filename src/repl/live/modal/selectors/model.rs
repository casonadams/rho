use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::super::ModalKeyResult;
use crate::error::Result;
use crate::repl::ReplSession;
use crate::ui::interactive::{ModalOption, ModalState, TerminalBackend, TerminalController};

fn canonical_model_id(item: &crate::repl::interactive::ModelItem) -> String {
    if item.id.contains('/') && item.id.starts_with(&format!("{}/", item.provider)) {
        item.id.clone()
    } else {
        format!("{}/{}", item.provider, item.id)
    }
}

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
    let canonical = canonical_model_id(item);
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

pub fn open_model_selector_with_default<B: TerminalBackend>(
    session: &ReplSession,
    controller: &mut TerminalController<B>,
    save_as_default: bool,
) {
    let discovered = crate::repl::interactive::discover_models(&session.config, &session.auth_store);
    let mut options = Vec::new();
    let mut initial_selection = 0;

    let active_spec = session.config.canonical_model_spec();
    for (i, item) in discovered.iter().enumerate() {
        let canonical = canonical_model_id(item);
        if canonical == active_spec || item.id == session.config.model {
            initial_selection = i;
        }
        options.push(build_model_option(session, item));
    }

    let mut modal = ModalState::new("Select Model", "", options)
        .with_search(true)
        .with_save_as_default(save_as_default);
    modal.selected = initial_selection;
    controller.state_mut().push_modal(modal);
}

pub fn open_role_model_selector<B: TerminalBackend>(
    session: &ReplSession,
    controller: &mut TerminalController<B>,
    role_name: &str,
    current_model: Option<&str>,
) {
    let discovered = crate::repl::interactive::discover_models(&session.config, &session.auth_store);
    let mut options = Vec::new();

    let none_mark = if current_model.is_none() { "✓" } else { "" };
    options.push(ModalOption::new(
        "None",
        Some(format!(
            "none\t{none_mark}\t\tDisable {role_name} model (fallback to default)"
        )),
    ));

    let mut initial_selection = 0;
    for (i, item) in discovered.iter().enumerate() {
        let canonical = canonical_model_id(item);
        let active_mark = if let Some(m) = current_model {
            let matches_full = m == canonical;
            let matches_id = m == item.id;
            if matches_full || matches_id {
                initial_selection = i + 1;
                "✓"
            } else {
                ""
            }
        } else {
            ""
        };
        options.push(ModalOption::new(
            canonical,
            Some(format!("{}\t{}\t\t{}", item.provider, active_mark, item.description)),
        ));
    }

    let title = format!("Select {role_name} Model");
    let mut modal = ModalState::new(title, "", options).with_search(true);
    modal.selected = initial_selection;
    controller.state_mut().push_modal(modal);
}

pub fn open_judge_model_selector<B: TerminalBackend>(session: &ReplSession, controller: &mut TerminalController<B>) {
    open_role_model_selector(session, controller, "Judge", session.config.judge_model());
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

pub fn open_plan_model_selector<B: TerminalBackend>(session: &ReplSession, controller: &mut TerminalController<B>) {
    let plan_model = session.config.models.get("plan").map(String::as_str);
    open_role_model_selector(session, controller, "Plan", plan_model);
}

pub fn open_commit_model_selector<B: TerminalBackend>(session: &ReplSession, controller: &mut TerminalController<B>) {
    let commit_model = session.config.models.get("commit").map(String::as_str);
    open_role_model_selector(session, controller, "Commit", commit_model);
}

pub fn open_advisor_model_selector<B: TerminalBackend>(session: &ReplSession, controller: &mut TerminalController<B>) {
    let advisor_model = session.config.models.get("advisor").map(String::as_str);
    open_role_model_selector(session, controller, "Advisor", advisor_model);
}

fn extract_selected_model<B: TerminalBackend>(controller: &TerminalController<B>) -> Option<(String, String)> {
    let opt = controller.state().active_modal().and_then(|m| m.selected_option())?;
    let label = opt.label.clone();
    if label == "None" {
        return Some(("None".to_string(), "none".to_string()));
    }
    let (provider, _) = rho_harness_core::provider::parse_model_spec(&label);
    if !provider.is_empty() {
        Some((label, provider))
    } else {
        let provider = opt
            .description
            .as_deref()
            .and_then(|d| d.split('\t').next())
            .unwrap_or("anthropic")
            .to_string();
        Some((label, provider))
    }
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
