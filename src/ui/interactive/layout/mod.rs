pub mod autocomplete;
pub mod editor;
pub mod modal;
#[cfg(test)]
mod tests;
pub mod text;

pub use text::{SPINNER_FRAMES, VisualTruncateResult, truncate_to_visual_lines, wrap_to_width};

use super::{Activity, AutocompleteState, EditorState, FooterState, ModalState};
use autocomplete::render_autocomplete_dropdown;
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
    pub widget_lines: Vec<String>,
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
        if !self.widget_lines.is_empty() {
            h += self.widget_lines.len() + 1; // widget lines + trailing blank spacer line
        }
        if !self.top_divider.is_empty() {
            h += 2; // dedicated status/spinner slot + top divider
        }
        if !self.bottom_divider.is_empty() {
            h += 1;
        }
        h
    }

    pub fn cursor_row(&self) -> usize {
        let mut row = self.queued_lines.len();
        if !self.widget_lines.is_empty() {
            row += self.widget_lines.len() + 1;
        }
        if !self.top_divider.is_empty() {
            row += 2; // dedicated status/spinner slot + top divider
        }
        row + self.cursor.row
    }
}

#[derive(Debug, Clone, Copy)]
pub struct LayoutInput<'a> {
    pub editor: &'a EditorState,
    pub modal: Option<&'a ModalState>,
    pub autocomplete: Option<&'a AutocompleteState>,
    pub footer: &'a FooterState,
    pub queued_messages: &'a [super::QueuedMessage],
    pub widget_lines: &'a [String],
    pub terminal_width: usize,
    pub spinner_frame: usize,
}

pub fn layout(input: LayoutInput<'_>) -> InteractiveLayout {
    let width = input.terminal_width.max(1);
    let queued_lines = queued_lines_text(input.queued_messages, width);
    let (working_line, widget_lines) = if input.modal.is_some() {
        (String::new(), Vec::new())
    } else {
        (
            working_line_text(input.footer, input.spinner_frame, width),
            input.widget_lines.to_vec(),
        )
    };

    let (editor_lines, cursor, top_divider, bottom_divider) = if let Some(modal) = input.modal {
        let (lines, cursor) = render_modal_overlay(modal, width);
        (lines, cursor, String::new(), String::new())
    } else {
        let (mut lines, cursor) = wrap_editor(input.editor, width);
        if let Some(ac) = input.autocomplete {
            let ac_lines = render_autocomplete_dropdown(ac, width);
            if !ac_lines.is_empty() {
                lines.extend(ac_lines);
            }
        }
        let (style, reset) = thinking_divider_style(input.footer.thinking_level.as_deref());
        let styled_divider = format!("{style}{}{reset}", "─".repeat(width));
        (lines, cursor, styled_divider.clone(), styled_divider)
    };

    let footer_lines = crate::ui::interactive::footer::format_footer_lines(input.footer, width);
    let footer = footer_lines.join("\n");

    InteractiveLayout {
        queued_lines,
        widget_lines,
        working_line,
        top_divider,
        editor_lines,
        bottom_divider,
        footer_lines,
        footer,
        cursor,
    }
}

pub fn thinking_divider_style(thinking_level: Option<&str>) -> (&'static str, &'static str) {
    match thinking_level.unwrap_or("off") {
        "off" => ("\x1b[2m", "\x1b[0m"),
        "minimal" => ("\x1b[90m", "\x1b[0m"),
        "low" => ("\x1b[34m", "\x1b[0m"),
        "medium" => ("\x1b[36m", "\x1b[0m"),
        "high" => ("\x1b[35m", "\x1b[0m"),
        "xhigh" => ("\x1b[31m", "\x1b[0m"),
        "max" => ("\x1b[1;31m", "\x1b[0m"),
        _ => ("\x1b[2m", "\x1b[0m"),
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

fn working_line_text(footer: &FooterState, spinner_frame: usize, width: usize) -> String {
    let activity = &footer.activity;
    let running_tool = footer.running_tool.as_deref();
    if (matches!(activity, Activity::Idle) && running_tool.is_none()) || width < 3 {
        return String::new();
    }
    let spinner = FRAMES[spinner_frame % FRAMES.len()];
    let accent = "\x1b[36m";
    let reset = "\x1b[0m";
    let dim = "\x1b[2m";
    let label = running_tool.unwrap_or("Working...");
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
