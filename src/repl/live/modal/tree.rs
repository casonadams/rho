use crate::error::Result;
use crate::ui::interactive::{
    InputAction, ModalMode, ModalOption, ModalState, TerminalBackend, TerminalController, map_key,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rho_harness_core::session::tree::SessionTree;

use super::{ModalKeyResult, apply_input_edit};

pub fn open_tree_selector<B: TerminalBackend>(tree: &SessionTree, controller: &mut TerminalController<B>) {
    let entries = crate::ui::interactive::tree_view::build_tree_display(tree);
    let mut options = Vec::new();
    let mut initial_selection = 0;

    for (i, entry) in entries.iter().enumerate() {
        if entry.is_active {
            initial_selection = i;
        }
        let marker = if entry.is_active { "●" } else { "○" };
        let indent = "  ".repeat(entry.depth);
        let label = format!("{indent}{marker} {}", entry.preview);
        let desc = if let Some(lbl) = &entry.label {
            format!("[{lbl}] {}", entry.id)
        } else {
            entry.id.clone()
        };
        options.push(ModalOption::new(label, Some(desc)));
    }

    let mut modal = ModalState::new("Conversation Tree", "Select a checkpoint to navigate to:", options);
    modal.selected = initial_selection;
    controller.state_mut().push_modal(modal);
}

fn selected_tree_node_id<B: TerminalBackend>(controller: &TerminalController<B>) -> Option<String> {
    let opt = controller.state().active_modal().and_then(|m| m.selected_option())?;
    let desc = opt.description.clone().unwrap_or_default();
    Some(desc.split_whitespace().last().unwrap_or(&desc).to_string())
}

fn pop_and_redraw_tree<B: TerminalBackend>(controller: &mut TerminalController<B>) -> Result<ModalKeyResult> {
    controller.state_mut().pop_modal();
    controller.redraw()?;
    Ok(ModalKeyResult::Handled)
}

fn enter_tree_label_mode<B: TerminalBackend>(controller: &mut TerminalController<B>) -> Result<ModalKeyResult> {
    if let Some(modal) = controller.state_mut().active_modal_mut() {
        modal.enter_input_mode("label");
    }
    controller.redraw()?;
    Ok(ModalKeyResult::Handled)
}

fn handle_tree_enter<B: TerminalBackend>(controller: &mut TerminalController<B>) -> Result<ModalKeyResult> {
    let Some(node_id) = selected_tree_node_id(controller) else {
        return pop_and_redraw_tree(controller);
    };
    controller.state_mut().pop_modal();
    Ok(ModalKeyResult::TreeNodeSelected { node_id })
}

fn apply_tree_edit<B: TerminalBackend>(controller: &mut TerminalController<B>, key: KeyEvent) -> Result<()> {
    match map_key(key) {
        InputAction::Clear => {
            if let Some(modal) = controller.state_mut().active_modal_mut() {
                modal.input.set_text("");
            }
        }
        InputAction::Edit(action) => {
            if let Some(modal) = controller.state_mut().active_modal_mut() {
                apply_input_edit(&mut modal.input, action);
            }
        }
        _ => {}
    }
    controller.redraw()?;
    Ok(())
}

fn handle_tree_select_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: &KeyEvent,
) -> Result<ModalKeyResult> {
    match key.code {
        KeyCode::Up | KeyCode::BackTab | KeyCode::Char('k') => {
            controller.state_mut().select_previous_modal_option();
            controller.redraw()?;
        }
        KeyCode::Down | KeyCode::Tab | KeyCode::Char('j') => {
            controller.state_mut().select_next_modal_option();
            controller.redraw()?;
        }
        KeyCode::Enter => return handle_tree_enter(controller),
        KeyCode::Esc | KeyCode::Char('c') => return pop_and_redraw_tree(controller),
        _ => {}
    }
    Ok(ModalKeyResult::Handled)
}

pub fn handle_tree_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
) -> Result<ModalKeyResult> {
    let is_input_mode = controller
        .state()
        .active_modal()
        .is_some_and(|m| matches!(m.mode, ModalMode::Input { .. }));
    if is_input_mode {
        return handle_tree_input_key(controller, key);
    }

    let label_shortcut =
        matches!(key.code, KeyCode::Char('l') | KeyCode::Char('L')) && key.modifiers.contains(KeyModifiers::SHIFT);
    if label_shortcut {
        return enter_tree_label_mode(controller);
    }
    handle_tree_select_key(controller, &key)
}

fn exit_tree_input_mode<B: TerminalBackend>(controller: &mut TerminalController<B>) -> Result<ModalKeyResult> {
    if let Some(modal) = controller.state_mut().active_modal_mut() {
        modal.mode = ModalMode::Select;
    }
    controller.redraw()?;
    Ok(ModalKeyResult::Handled)
}

fn submit_tree_label<B: TerminalBackend>(controller: &mut TerminalController<B>) -> Result<ModalKeyResult> {
    let input_text = controller
        .state()
        .active_modal()
        .map(|m| m.input.text().trim().to_string())
        .unwrap_or_default();
    let Some(node_id) = selected_tree_node_id(controller) else {
        controller.state_mut().pop_modal();
        return Ok(ModalKeyResult::Handled);
    };
    controller.state_mut().pop_modal();
    Ok(ModalKeyResult::NodeLabelUpdated {
        node_id,
        label: input_text,
    })
}

fn handle_tree_input_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
) -> Result<ModalKeyResult> {
    match key.code {
        KeyCode::Esc => exit_tree_input_mode(controller),
        KeyCode::Enter => submit_tree_label(controller),
        _ => {
            apply_tree_edit(controller, key)?;
            Ok(ModalKeyResult::Handled)
        }
    }
}
