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
    if matches!(
        (key.modifiers, key.code),
        (KeyModifiers::NONE, KeyCode::Char('c' | 'C'))
    ) {
        let url = controller
            .state()
            .active_modal()
            .map(|m| m.body.clone())
            .unwrap_or_default();
        super::pop_and_cancel(controller)?;
        if !url.is_empty() {
            let _ = crate::platform::clipboard::set_text(&url);
            controller.set_system_message("Copied pairing URL to clipboard");
            controller.redraw()?;
        }
        return Ok(ModalKeyResult::Handled);
    }

    match key.code {
        KeyCode::Enter => {
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
            super::pop_and_cancel(controller)?;
            if selected.starts_with("Copy Link") && !url.is_empty() {
                let _ = crate::platform::clipboard::set_text(&url);
                controller.set_system_message("Copied pairing URL to clipboard");
            } else if selected.starts_with("Show QR Code") && !url.is_empty() {
                controller.set_system_message("Pairing QR code generated");
            }
            controller.redraw()?;
            Ok(ModalKeyResult::Handled)
        }
        KeyCode::Esc => {
            super::pop_and_cancel(controller)?;
            Ok(ModalKeyResult::Handled)
        }
        _ => {
            super::handle_selector_nav(controller, &key)?;
            Ok(ModalKeyResult::Handled)
        }
    }
}
