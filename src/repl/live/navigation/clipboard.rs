use crate::repl::ReplSession;
use crate::ui::interactive::{TerminalBackend, TerminalController};

pub fn copy_last_message<B: TerminalBackend>(session: &ReplSession, controller: &TerminalController<B>) {
    let last_text = controller.transcript().iter().rev().find_map(|item| match item {
        crate::ui::interactive::TranscriptItem::AssistantText(text) => Some(text.clone()),
        _ => None,
    });

    if let Some(text) = last_text {
        if crate::platform::clipboard::set_text(&text).is_ok() {
            session.renderer.print_status("Copied message to clipboard");
        } else {
            session.renderer.print_status("Failed to access clipboard");
        }
    } else {
        session.renderer.print_status("No assistant message to copy");
    }
}

pub fn paste_clipboard<B: TerminalBackend>(
    _renderer: &crate::ui::TerminalRenderer,
    controller: &mut TerminalController<B>,
) {
    if let Some(text) = crate::platform::clipboard::get_text_or_image_path()
        && !crate::repl::live::modal::handle_modal_paste(controller, &text)
    {
        controller.state_mut().editor_mut().handle_paste(&text);
    }
}
