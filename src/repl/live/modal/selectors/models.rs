use super::super::ModalKeyResult;
use crate::error::Result;
use crate::repl::ReplSession;
use crate::ui::interactive::{ModalOption, ModalState, TerminalBackend, TerminalController};
use crossterm::event::{KeyCode, KeyEvent};

pub fn build_models_options(session: &ReplSession) -> Vec<ModalOption> {
    let cfg = &session.config;
    let default_m = cfg.model.as_str();
    let smol_m = cfg.models.get("smol").map(String::as_str).unwrap_or("None");
    let slow_m = cfg.models.get("slow").map(String::as_str).unwrap_or("None");
    let guard_m = cfg.guard_model().unwrap_or("None");

    vec![
        ModalOption::new("Default Model       ", Some(default_m.to_string())),
        ModalOption::new("Smol Model          ", Some(smol_m.to_string())),
        ModalOption::new("Slow Model          ", Some(slow_m.to_string())),
        ModalOption::new("Guard Model         ", Some(guard_m.to_string())),
    ]
}

pub fn open_models_selector<B: TerminalBackend>(session: &ReplSession, controller: &mut TerminalController<B>) {
    let options = build_models_options(session);
    let modal = ModalState::new("Models", "", options);
    controller.state_mut().push_modal(modal);
}

fn open_role_from_selection(selected: usize) -> Option<ModalKeyResult> {
    let selectors = [
        ModalKeyResult::OpenModelSelector { save_as_default: true },
        ModalKeyResult::OpenSmolModelSelector,
        ModalKeyResult::OpenSlowModelSelector,
        ModalKeyResult::OpenGuardModelSelector,
    ];
    selectors.into_iter().nth(selected)
}

fn role_to_index(role: &str) -> Option<usize> {
    match role {
        "default" => Some(0),
        "smol" => Some(1),
        "slow" => Some(2),
        "guard" => Some(3),
        _ => None,
    }
}

pub fn update_models_role_description<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    role: &str,
    model_name: &str,
) {
    if let Some(idx) = role_to_index(role) {
        controller
            .state_mut()
            .update_modal_option_desc("Models", idx, model_name);
    }
}

pub fn handle_models_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
) -> Result<ModalKeyResult> {
    match key.code {
        KeyCode::Enter => {
            let selected = controller.state().active_modal().map_or(0, |m| m.selected);
            if let Some(res) = open_role_from_selection(selected) {
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
        let session = create_test_session();
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
            ]
        );
    }

    #[test]
    fn update_models_role_description_updates_only_known_roles() {
        let cases = [
            ("default", Some(0)),
            ("smol", Some(1)),
            ("slow", Some(2)),
            ("guard", Some(3)),
            ("plan", None),
        ];
        for (role, idx) in cases {
            let mut controller = open_in_test_controller(open_models_selector);
            update_models_role_description(&mut controller, role, "model-x");
            let options = &controller.state().active_modal().unwrap().options;
            let updated: Vec<usize> = options
                .iter()
                .enumerate()
                .filter(|(_, o)| o.description.as_deref() == Some("model-x"))
                .map(|(i, _)| i)
                .collect();
            assert_eq!(updated, idx.map_or(vec![], |i| vec![i]));
        }
    }

    #[test]
    fn open_models_modal_and_select() {
        assert_selector_opens_and_selects(
            open_models_selector,
            handle_models_key,
            "Models",
            4,
            ModalKeyResult::OpenModelSelector { save_as_default: true },
            false,
        );
    }

    #[test]
    fn test_open_role_from_selection_all_roles() {
        for idx in 0..4 {
            assert!(open_role_from_selection(idx).is_some());
        }
        assert!(open_role_from_selection(4).is_none());
    }
}
