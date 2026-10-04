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
    let judge_m = cfg.judge_model().unwrap_or("None");
    let guard_m = cfg.guard_model().unwrap_or("None");
    let plan_m = cfg.models.get("plan").map(String::as_str).unwrap_or("None");
    let commit_m = cfg.models.get("commit").map(String::as_str).unwrap_or("None");
    let advisor_m = cfg.models.get("advisor").map(String::as_str).unwrap_or("None");

    vec![
        ModalOption::new("Default Model       ", Some(default_m.to_string())),
        ModalOption::new("Smol Model          ", Some(smol_m.to_string())),
        ModalOption::new("Slow Model          ", Some(slow_m.to_string())),
        ModalOption::new("Judge Model         ", Some(judge_m.to_string())),
        ModalOption::new("Guard Model         ", Some(guard_m.to_string())),
        ModalOption::new("Plan Model          ", Some(plan_m.to_string())),
        ModalOption::new("Commit Model        ", Some(commit_m.to_string())),
        ModalOption::new("Advisor Model       ", Some(advisor_m.to_string())),
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
        ModalKeyResult::OpenJudgeModelSelector,
        ModalKeyResult::OpenGuardModelSelector,
        ModalKeyResult::OpenPlanModelSelector,
        ModalKeyResult::OpenCommitModelSelector,
        ModalKeyResult::OpenAdvisorModelSelector,
    ];
    selectors.into_iter().nth(selected)
}

fn role_to_index(role: &str) -> Option<usize> {
    match role {
        "default" => Some(0),
        "smol" => Some(1),
        "slow" => Some(2),
        "judge" => Some(3),
        "guard" => Some(4),
        "plan" => Some(5),
        "commit" => Some(6),
        "advisor" => Some(7),
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
    use super::*;
    use crate::ui::interactive::InteractiveState;
    use std::io;

    struct DummyTerminal;
    impl TerminalBackend for DummyTerminal {
        fn set_raw_mode(&mut self, _enabled: bool) -> io::Result<()> {
            Ok(())
        }
        fn size(&self) -> io::Result<(u16, u16)> {
            Ok((80, 24))
        }
        fn hide_cursor(&mut self) -> io::Result<()> {
            Ok(())
        }
        fn show_cursor(&mut self) -> io::Result<()> {
            Ok(())
        }
        fn move_up(&mut self, _rows: usize) -> io::Result<()> {
            Ok(())
        }
        fn move_down(&mut self, _rows: usize) -> io::Result<()> {
            Ok(())
        }
        fn move_to_column(&mut self, _column: usize) -> io::Result<()> {
            Ok(())
        }
        fn clear_line(&mut self) -> io::Result<()> {
            Ok(())
        }
        fn write_text(&mut self, _text: &str) -> io::Result<()> {
            Ok(())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn create_test_session() -> ReplSession {
        ReplSession::new(
            rho_harness_core::config::Config::default(),
            rho_engine::auth::AuthStore::default(),
            None,
        )
    }

    #[test]
    fn build_models_options_lists_all_roles() {
        let session = create_test_session();
        let options = build_models_options(&session);
        assert_eq!(options.len(), 8);
        assert_eq!(options[0].label, "Default Model       ");
        assert_eq!(options[1].label, "Smol Model          ");
        assert_eq!(options[2].label, "Slow Model          ");
        assert_eq!(options[3].label, "Judge Model         ");
        assert_eq!(options[4].label, "Guard Model         ");
    }

    #[test]
    fn open_models_modal_and_select() {
        let session = create_test_session();
        let mut controller = TerminalController::new(DummyTerminal, InteractiveState::default()).unwrap();
        open_models_selector(&session, &mut controller);

        assert!(controller.state().active_modal().is_some());
        assert_eq!(controller.state().active_modal().unwrap().title, "Models");

        let enter = KeyEvent::new(KeyCode::Enter, crossterm::event::KeyModifiers::NONE);
        let res = handle_models_key(&mut controller, enter).unwrap();
        assert_eq!(res, ModalKeyResult::OpenModelSelector { save_as_default: true });
        assert!(controller.state().active_modal().is_some());
    }

    #[test]
    fn test_open_role_from_selection_all_roles() {
        for idx in 0..8 {
            assert!(open_role_from_selection(idx).is_some());
        }
        assert!(open_role_from_selection(8).is_none());
    }
}
