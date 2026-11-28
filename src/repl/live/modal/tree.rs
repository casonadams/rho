use crate::error::Result;
use crate::ui::interactive::{ModalKind, ModalMode, ModalOption, ModalState, TerminalBackend, TerminalController};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rho_harness_core::session::tree::SessionTree;

use super::{ModalKeyResult, apply_modal_edit_key};

pub fn open_tree_selector<B: TerminalBackend>(tree: &SessionTree, controller: &mut TerminalController<B>) {
    let entries = crate::tree_display::build_tree_display(tree);
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
        options.push(ModalOption::new(label, Some(desc)).with_value(entry.id.clone()));
    }

    let mut modal = ModalState::new("Conversation Tree", "", options)
        .with_search(true)
        .with_kind(ModalKind::Tree);
    modal.selected = initial_selection;
    controller.state_mut().push_modal(modal);
}

fn selected_tree_node_id<B: TerminalBackend>(controller: &TerminalController<B>) -> Option<String> {
    controller.state().active_modal()?.selected_option()?.value.clone()
}

fn enter_tree_label_mode<B: TerminalBackend>(controller: &mut TerminalController<B>) -> Result<ModalKeyResult> {
    if let Some(modal) = controller.state_mut().active_modal_mut() {
        modal.enter_input_mode("label");
    }
    controller.redraw()?;
    Ok(ModalKeyResult::Handled)
}

fn handle_tree_select_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
) -> Result<ModalKeyResult> {
    super::dispatch_simple_selector(controller, key, |opt| {
        opt.value
            .clone()
            .map(|node_id| ModalKeyResult::TreeNodeSelected { node_id })
    })
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

    let label_shortcut = key.code == KeyCode::Char('l') && key.modifiers.contains(KeyModifiers::CONTROL);
    if label_shortcut {
        return enter_tree_label_mode(controller);
    }
    handle_tree_select_key(controller, key)
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
        .map(|m| m.input.expanded_text().trim().to_string())
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
            apply_modal_edit_key(controller, key);
            controller.redraw()?;
            Ok(ModalKeyResult::Handled)
        }
    }
}
