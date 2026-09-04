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
    renderer: &crate::ui::TerminalRenderer,
    controller: &mut TerminalController<B>,
) {
    if let Ok(Some(img)) = crate::platform::clipboard::get_image() {
        match crate::platform::clipboard::save_image_to_temp_png(&img) {
            Ok(path) => {
                let path_str = path.to_string_lossy();
                controller.state_mut().editor_mut().handle_paste(&path_str);
            }
            Err(error) => {
                renderer.print_notice(&format!("  [Failed to save clipboard image: {error}]\n"));
            }
        }
    } else if let Ok(Some(text)) = crate::platform::clipboard::get_text() {
        controller.state_mut().editor_mut().handle_paste(&text);
    }
}
