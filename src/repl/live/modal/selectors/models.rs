use super::super::ModalKeyResult;
use crate::error::Result;
use crate::repl::ReplSession;
use crate::ui::interactive::{ModalOption, ModalState, TerminalBackend, TerminalController};
use crossterm::event::{KeyCode, KeyEvent};
use rho_harness_core::config::ModelSlot;

pub fn build_models_options(session: &ReplSession) -> Vec<ModalOption> {
    let cfg = &session.config;
    let default = ModalOption::new(format!("{:20}", "Default Model"), Some(cfg.canonical_model_spec()));
    let slots = ModelSlot::ALL.into_iter().map(|slot| {
        let model = cfg.slot_model(slot).unwrap_or("None");
        ModalOption::new(
            format!("{:20}", format!("{} Model", slot.title())),
            Some(model.to_string()),
        )
    });
    std::iter::once(default).chain(slots).collect()
}

pub fn open_models_selector<B: TerminalBackend>(session: &ReplSession, controller: &mut TerminalController<B>) {
    let options = build_models_options(session);
    let modal = ModalState::new("Models", "", options);
    controller.state_mut().push_modal(modal);
}

/// Row 0 is the default model; the slots follow in `ModelSlot::ALL` order.
fn open_role_from_selection(selected: usize) -> Option<ModalKeyResult> {
    match selected.checked_sub(1) {
        None => Some(ModalKeyResult::OpenModelSelector { save_as_default: true }),
        Some(i) => ModelSlot::ALL
            .get(i)
            .map(|&slot| ModalKeyResult::OpenSlotModelSelector { slot }),
    }
}

/// Updates the Models menu row for `slot`, or the default model row for `None`.
pub fn update_models_role_description<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    slot: Option<ModelSlot>,
    model_name: &str,
) {
    let idx = slot.map_or(0, |slot| {
        1 + ModelSlot::ALL.iter().position(|&s| s == slot).unwrap_or(0)
    });
    controller
        .state_mut()
        .update_modal_option_desc("Models", idx, model_name);
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
    fn update_models_role_description_updates_the_matching_row() {
        let cases = [
            (None, 0),
            (Some(ModelSlot::Smol), 1),
            (Some(ModelSlot::Slow), 2),
            (Some(ModelSlot::Guard), 3),
        ];
        for (slot, idx) in cases {
            let mut controller = open_in_test_controller(open_models_selector);
            update_models_role_description(&mut controller, slot, "model-x");
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
