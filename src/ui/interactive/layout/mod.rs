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
use text::{SPINNER_FRAMES as FRAMES, truncate_to_width};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorPosition {
    pub row: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InteractiveLayout {
    pub lines: Vec<String>,
    pub cursor: CursorPosition,
    pub cursor_visible: bool,
    pub queued_lines: Vec<String>,
    pub widget_lines: Vec<String>,
    pub working_line: String,
    pub top_divider: String,
    pub editor_lines: Vec<String>,
    pub bottom_divider: String,
    pub footer_lines: Vec<String>,
    pub footer: String,
}

impl InteractiveLayout {
    pub fn height(&self) -> usize {
        self.lines.len()
    }

    pub fn cursor_row(&self) -> usize {
        let mut row = self.queued_lines.len();
        if !self.widget_lines.is_empty() {
            row += self.widget_lines.len() + 1;
        }
        if !self.top_divider.is_empty() {
            row += 3;
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
    let mut lines = Vec::new();

    let queued_lines = queued_lines_text(input.queued_messages, width);
    lines.extend(queued_lines.clone());

    let (working_line, widget_lines) = if input.modal.is_some() {
        (String::new(), Vec::new())
    } else {
        (
            working_line_text(input.footer, input.spinner_frame, width),
            input.widget_lines.to_vec(),
        )
    };

    if !widget_lines.is_empty() {
        lines.extend(widget_lines.clone());
        lines.push(String::new());
    }

    let (editor_lines, top_divider, bottom_divider, footer_lines, cursor, cursor_visible) =
        if let Some(modal) = input.modal {
            let (modal_lines, modal_cursor, modal_cursor_visible) = render_modal_overlay(modal, width);
            lines.extend(modal_lines.clone());
            (
                modal_lines,
                String::new(),
                String::new(),
                Vec::new(),
                modal_cursor,
                modal_cursor_visible,
            )
        } else {
            lines.push(String::new());
            lines.push(working_line.clone());

            let (style, reset) = thinking_divider_style(input.footer.thinking_level.as_deref());
            let top_div = format!("{style}{}{reset}", "─".repeat(width));
            lines.push(top_div.clone());

            let (mut ed_lines, ed_cursor) = wrap_editor(input.editor, width);
            if let Some(ac) = input.autocomplete {
                let ac_lines = render_autocomplete_dropdown(ac, width);
                if !ac_lines.is_empty() {
                    ed_lines.extend(ac_lines);
                }
            }
            lines.extend(ed_lines.clone());

            let bot_div = format!("{style}{}{reset}", "─".repeat(width));
            lines.push(bot_div.clone());

            let ft_lines = crate::ui::interactive::footer::format_footer_lines(input.footer, width);
            let footer_style = crate::ui::theme::Theme::default().dimmed;
            for fl in &ft_lines {
                lines.push(format!("{footer_style}{fl}{footer_style:#}"));
            }
            (ed_lines, top_div, bot_div, ft_lines, ed_cursor, true)
        };

    let footer = footer_lines.join("\n");

    InteractiveLayout {
        lines,
        cursor,
        cursor_visible,
        queued_lines,
        widget_lines,
        working_line,
        top_divider,
        editor_lines,
        bottom_divider,
        footer_lines,
        footer,
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
            super::QueueKind::Steering if item.text.starts_with('/') => "Command",
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
    let full = format!("{accent}{spinner}{reset} {dim}Working...{reset}");
    truncate_to_width(&full, width)
}
