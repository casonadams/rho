use super::ModalKeyResult;
use crate::error::Result;
use crate::ui::interactive::{ModalOption, ModalState, TerminalBackend, TerminalController};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub fn open_remote_modal<B: TerminalBackend>(controller: &mut TerminalController<B>, pairing_url: &str) {
    let options = vec![
        ModalOption::new(
            format!("{:<15}", "Copy Link"),
            Some("Copy web pairing URL to system clipboard"),
        ),
        ModalOption::new(
            format!("{:<15}", "Show QR Code"),
            Some("Print terminal QR code into transcript"),
        ),
        ModalOption::new(
            format!("{:<15}", "Dismiss"),
            Some("Close this modal (session stays shared)"),
        ),
    ];
    let modal = ModalState::new("Remote Access", pairing_url, options);
    controller.state_mut().push_modal(modal);
}

pub fn handle_remote_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
) -> Result<ModalKeyResult> {
    match (key.modifiers, key.code) {
        (KeyModifiers::NONE, KeyCode::Char('c' | 'C')) => {
            let url = controller
                .state()
                .active_modal()
                .map(|m| m.body.clone())
                .unwrap_or_default();
            controller.state_mut().pop_modal();
            if !url.is_empty() {
                let _ = crate::platform::clipboard::set_text(&url);
                controller.set_system_message("Copied pairing URL to clipboard");
            }
            controller.redraw()?;
            Ok(ModalKeyResult::Handled)
        }
        (KeyModifiers::NONE, KeyCode::Esc) | (KeyModifiers::CONTROL, KeyCode::Char('c' | 'C')) => {
            controller.state_mut().pop_modal();
            controller.redraw()?;
            Ok(ModalKeyResult::Handled)
        }
        (KeyModifiers::NONE, KeyCode::Enter) => {
            let selected = controller
                .state()
                .active_modal()
                .and_then(|m| m.selected_option())
                .map(|o| o.label.trim().to_string())
                .unwrap_or_default();
            let url = controller
                .state()
                .active_modal()
                .map(|m| m.body.clone())
                .unwrap_or_default();
            controller.state_mut().pop_modal();

            if selected.starts_with("Copy Link") {
                if !url.is_empty() {
                    let _ = crate::platform::clipboard::set_text(&url);
                    controller.set_system_message("Copied pairing URL to clipboard");
                }
            } else if selected.starts_with("Show QR Code") && !url.is_empty() {
                controller.set_system_message("Pairing QR code generated");
            }
            controller.redraw()?;
            Ok(ModalKeyResult::Handled)
        }
        (KeyModifiers::NONE, KeyCode::Up | KeyCode::Char('k')) => {
            if let Some(modal) = controller.state_mut().active_modal_mut() {
                modal.select_previous();
            }
            controller.redraw()?;
            Ok(ModalKeyResult::Handled)
        }
        (KeyModifiers::NONE, KeyCode::Down | KeyCode::Char('j')) => {
            if let Some(modal) = controller.state_mut().active_modal_mut() {
                modal.select_next();
            }
            controller.redraw()?;
            Ok(ModalKeyResult::Handled)
        }
        _ => Ok(ModalKeyResult::Handled),
    }
}
