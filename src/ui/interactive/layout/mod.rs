pub mod active_tool;
pub mod editor;
pub mod modal;
#[cfg(test)]
mod tests;
pub mod text;

pub use active_tool::{ActiveToolDisplayInput, format_active_tool_block};
pub use text::{SPINNER_FRAMES, VisualTruncateResult, truncate_to_visual_lines, wrap_to_width};

use super::{Activity, EditorState, FooterState, ModalState};
use editor::wrap_editor;
use modal::render_modal_overlay;
use text::{SPINNER_FRAMES as FRAMES, truncate_to_width, visible_width as calc_visible_width};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorPosition {
    pub row: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InteractiveLayout {
    pub queued_lines: Vec<String>,
    pub working_line: String,
    pub top_divider: String,
    pub editor_lines: Vec<String>,
    pub bottom_divider: String,
    pub footer_lines: Vec<String>,
    pub footer: String,
    pub cursor: CursorPosition,
}

impl InteractiveLayout {
    pub fn height(&self) -> usize {
        let mut h = self.editor_lines.len() + self.queued_lines.len() + self.footer_lines.len();
        if !self.working_line.is_empty() {
            h += 2; // blank line above working line + working line itself
        }
        if !self.top_divider.is_empty() {
            h += 1;
        }
        if !self.bottom_divider.is_empty() {
            h += 1;
        }
        h
    }

    pub fn cursor_row(&self) -> usize {
        let mut row = self.queued_lines.len();
        if !self.working_line.is_empty() {
            row += 2;
        }
        if !self.top_divider.is_empty() {
            row += 1;
        }
        row + self.cursor.row
    }
}

#[derive(Debug, Clone, Copy)]
pub struct LayoutInput<'a> {
    pub editor: &'a EditorState,
    pub modal: Option<&'a ModalState>,
    pub footer: &'a FooterState,
    pub queued_messages: &'a [super::QueuedMessage],
    pub terminal_width: usize,
    pub spinner_frame: usize,
}

pub fn layout(input: LayoutInput<'_>) -> InteractiveLayout {
    let width = input.terminal_width.max(1);
    let queued_lines = queued_lines_text(input.queued_messages, width);
    let working_line = if input.modal.is_some() {
        String::new()
    } else {
        working_line_text(&input.footer.activity, input.spinner_frame, width)
    };

    let (editor_lines, cursor, top_divider, bottom_divider) = if let Some(modal) = input.modal {
        let (lines, cursor) = render_modal_overlay(modal, width);
        let divider = truncate_to_width(&"─".repeat(width), width);
        (lines, cursor, String::new(), divider)
    } else {
        let (lines, cursor) = wrap_editor(input.editor, width);
        let divider = truncate_to_width(&"─".repeat(width), width);
        (lines, cursor, divider.clone(), divider)
    };

    let footer_lines = crate::ui::interactive::footer::format_footer_lines(input.footer, width);
    let footer = footer_lines.join("\n");

    InteractiveLayout {
        queued_lines,
        working_line,
        top_divider,
        editor_lines,
        bottom_divider,
        footer_lines,
        footer,
        cursor,
    }
}

fn queued_lines_text(queued: &[super::QueuedMessage], width: usize) -> Vec<String> {
    if queued.is_empty() || width < 12 {
        return Vec::new();
    }
    let dim = "\x1b[2m";
    let reset = "\x1b[0m";
    let accent = "\x1b[36m";
    let mut lines = Vec::new();
    for item in queued {
        let kind_label = match item.kind {
            super::QueueKind::Steering => "Steering",
            super::QueueKind::FollowUp => "Follow-up",
        };
        let text = format!("{dim}⇣ {kind_label}: {}{reset}", item.text.replace('\n', " "));
        lines.push(truncate_to_width(&text, width));
    }
    let hint = format!("{dim}↳ {accent}Alt+↑{reset}{dim} to edit queued messages{reset}");
    lines.push(truncate_to_width(&hint, width));
    lines
}

fn working_line_text(activity: &Activity, spinner_frame: usize, width: usize) -> String {
    if matches!(activity, Activity::Idle) || width < 3 {
        return String::new();
    }
    let spinner = FRAMES[spinner_frame % FRAMES.len()];
    let accent = "\x1b[36m";
    let reset = "\x1b[0m";
    let dim = "\x1b[2m";
    let label = "Working...";
    let full = format!("{accent}{spinner}{reset} {dim}{label}{reset}");
    if calc_visible_width(&full) <= width {
        full
    } else {
        let label_width = width.saturating_sub(2);
        let shown = truncate_to_width(label, label_width);
        let dots = width.saturating_sub(calc_visible_width(&shown) + 2).min(3);
        format!("{accent}{spinner}{reset} {dim}{}{}{reset}", shown, ".".repeat(dots))
    }
}
