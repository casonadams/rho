use super::super::ModalKeyResult;
use crate::error::Result;
use crate::repl::ReplSession;
use crate::ui::interactive::{ModalOption, ModalState, TerminalBackend, TerminalController};
use crossterm::event::{KeyCode, KeyEvent};
use rho_harness_core::config::{Config, ModelRole};

pub fn visible_model_roles(config: &Config) -> Vec<ModelRole> {
    let mut roles = ModelRole::BUILTINS.to_vec();
    for key in config.model_roles.keys() {
        if let Ok(role) = ModelRole::parse(key)
            && !roles.contains(&role)
        {
            roles.push(role);
        }
    }
    roles
}

pub fn build_models_options(session: &ReplSession) -> Vec<ModalOption> {
    let cfg = &session.config;
    let default = ModalOption::new(format!("{:20}", "Default Model"), Some(cfg.canonical_model_spec()));
    let roles = visible_model_roles(cfg).into_iter().map(|role| {
        let model = cfg.role_model(role.as_str()).unwrap_or("None");
        ModalOption::new(
            format!("{:20}", format!("{} Model", role.title())),
            Some(model.to_string()),
        )
    });
    std::iter::once(default).chain(roles).collect()
}

pub fn open_models_selector<B: TerminalBackend>(session: &ReplSession, controller: &mut TerminalController<B>) {
    let options = build_models_options(session);
    let modal = ModalState::new("Models", "", options);
    controller.state_mut().push_modal(modal);
}

/// Row 0 is the default model; the roles follow dynamically.
fn open_role_from_selection<B: TerminalBackend>(
    controller: &TerminalController<B>,
    selected: usize,
) -> Option<ModalKeyResult> {
    if selected == 0 {
        return Some(ModalKeyResult::OpenModelSelector { save_as_default: true });
    }
    let modal = controller.state().active_modal()?;
    let option = modal.options.get(selected)?;
    let raw = option.label.trim().strip_suffix(" Model")?;
    let role = ModelRole::parse(raw).ok()?;
    Some(ModalKeyResult::OpenRoleModelSelector { role })
}

/// Updates the Models menu row for `role`, or the default model row for `None`.
pub fn update_models_role_description<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    role: Option<&ModelRole>,
    model_name: &str,
) {
    let Some(modal) = controller.state_mut().find_modal_mut("Models") else {
        return;
    };
    let target_label = role.map(|r| format!("{} Model", r.title()));
    let idx = match target_label.as_deref() {
        None => Some(0),
        Some(label) => modal.options.iter().position(|opt| opt.label.trim() == label),
    };
    if let Some(idx) = idx
        && let Some(opt) = modal.options.get_mut(idx)
    {
        opt.description = Some(model_name.to_string());
    }
}

pub fn handle_models_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
) -> Result<ModalKeyResult> {
    match key.code {
        KeyCode::Enter => {
            let selected = controller.state().active_modal().map_or(0, |m| m.selected);
            if let Some(res) = open_role_from_selection(controller, selected) {
                return Ok(res);
            }
            Ok(ModalKeyResult::Handled)
        }
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

#[cfg(test)]
mod tests {
    use super::super::test_support::{assert_selector_opens_and_selects, create_test_session, open_in_test_controller};
    use super::*;

    #[test]
    fn build_models_options_lists_all_roles() {
        let mut session = create_test_session();
        session
            .config
            .model_roles
            .insert("custom_role".to_string(), "openai/gpt-4o".to_string());
        let labels: Vec<String> = build_models_options(&session)
            .into_iter()
            .map(|option| option.label)
            .collect();
        assert_eq!(
            labels,
            [
                "Default Model       ",
                "Smol Model          ",
                "Slow Model          ",
                "Guard Model         ",
                "Judge Model         ",
                "Widget Model        ",
                "Review Model        ",
                "Plan Model          ",
                "Task Model          ",
                "Vision Model        ",
                "Custom_role Model   ",
            ]
        );
    }

    #[test]
    fn update_models_role_description_updates_the_matching_row() {
        let cases = [
            (None, 0),
            (Some(ModelRole::SMOL), 1),
            (Some(ModelRole::SLOW), 2),
            (Some(ModelRole::GUARD), 3),
            (Some(ModelRole::WIDGET), 5),
        ];
        for (role, idx) in cases {
            let mut controller = open_in_test_controller(open_models_selector);
            update_models_role_description(&mut controller, role.as_ref(), "model-x");
            let options = &controller.state().active_modal().unwrap().options;
            let updated: Vec<usize> = options
                .iter()
                .enumerate()
                .filter(|(_, o)| o.description.as_deref() == Some("model-x"))
                .map(|(i, _)| i)
                .collect();
            assert_eq!(updated, vec![idx]);
        }
    }

    #[test]
    fn open_models_modal_and_select() {
        assert_selector_opens_and_selects(
            open_models_selector,
            handle_models_key,
            "Models",
            1 + ModelRole::BUILTINS.len(),
            ModalKeyResult::OpenModelSelector { save_as_default: true },
            false,
        );
    }

    #[test]
    fn test_open_role_from_selection_all_roles() {
        let controller = open_in_test_controller(open_models_selector);
        let count = 1 + ModelRole::BUILTINS.len();
        for idx in 0..count {
            assert!(open_role_from_selection(&controller, idx).is_some());
        }
        assert!(open_role_from_selection(&controller, count).is_none());
    }

    #[test]
    fn custom_role_selection_and_description_update_work_dynamically() {
        let mut session = create_test_session();
        session
            .config
            .model_roles
            .insert("custom-role".to_string(), "openai/gpt-4o".to_string());
        let mut controller = open_in_test_controller(|_, ctrl| open_models_selector(&session, ctrl));
        let custom = ModelRole::parse("custom-role").unwrap();
        let custom_idx = 1 + ModelRole::BUILTINS.len();

        assert_eq!(
            open_role_from_selection(&controller, custom_idx),
            Some(ModalKeyResult::OpenRoleModelSelector { role: custom.clone() })
        );
        update_models_role_description(&mut controller, Some(&custom), "anthropic/claude-sonnet-4-5");
        assert_eq!(
            controller.state().active_modal().unwrap().options[custom_idx]
                .description
                .as_deref(),
            Some("anthropic/claude-sonnet-4-5")
        );
    }
}
