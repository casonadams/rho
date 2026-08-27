use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::{EditorState, FooterState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorPosition {
    pub row: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InteractiveLayout {
    pub top_divider: String,
    pub editor_lines: Vec<String>,
    pub bottom_divider: String,
    pub footer: String,
    pub cursor: CursorPosition,
}

impl InteractiveLayout {
    pub fn height(&self) -> usize {
        self.editor_lines.len() + 3
    }
}

pub struct LayoutInput<'a> {
    pub editor: &'a EditorState,
    pub footer: &'a FooterState,
    pub queued_messages: usize,
    pub terminal_width: usize,
}

pub fn layout(input: LayoutInput<'_>) -> InteractiveLayout {
    let width = input.terminal_width.max(1);
    let (editor_lines, cursor) = wrap_editor(input.editor, width);
    let divider = "─".repeat(width);

    InteractiveLayout {
        top_divider: divider.clone(),
        editor_lines,
        bottom_divider: divider,
        footer: truncate_to_width(&footer_text(input.footer, input.queued_messages), width),
        cursor,
    }
}

fn footer_text(footer: &FooterState, queued_messages: usize) -> String {
    let mut segments = vec![footer.activity.label().to_string()];
    if !footer.model.is_empty() {
        segments.push(footer.model.clone());
    }
    if let Some(context) = footer.context.as_deref().filter(|value| !value.is_empty()) {
        segments.push(context.to_string());
    }
    if let Some(quota) = footer.quota.as_deref().filter(|value| !value.is_empty()) {
        segments.push(quota.to_string());
    }
    if queued_messages > 0 {
        segments.push(format!("{queued_messages} queued"));
    }
    segments.join(" | ")
}

fn wrap_editor(editor: &EditorState, width: usize) -> (Vec<String>, CursorPosition) {
    let mut lines = vec![String::new()];
    let mut row = 0;
    let mut column = 0;
    let mut cursor = None;

    for (byte_index, character) in editor.text().char_indices() {
        if character == '\n' {
            if byte_index == editor.cursor() {
                cursor = Some(CursorPosition { row, column });
            }
            lines.push(String::new());
            row += 1;
            column = 0;
            continue;
        }

        let character_width = character.width().unwrap_or(0);
        if column > 0 && column + character_width > width {
            lines.push(String::new());
            row += 1;
            column = 0;
        }
        if byte_index == editor.cursor() {
            cursor = Some(CursorPosition { row, column });
        }
        lines[row].push(character);
        column += character_width;
    }

    if editor.cursor() == editor.text().len() {
        if column == width {
            lines.push(String::new());
            row += 1;
            column = 0;
        }
        cursor = Some(CursorPosition { row, column });
    }

    (
        lines,
        cursor.expect("editor cursor must be on a UTF-8 character boundary"),
    )
}

fn truncate_to_width(value: &str, width: usize) -> String {
    if value.width() <= width {
        return value.to_string();
    }

    let mut result = String::new();
    let mut used = 0;
    for character in value.chars() {
        let character_width = character.width().unwrap_or(0);
        if used + character_width > width {
            break;
        }
        result.push(character);
        used += character_width;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::{CursorPosition, LayoutInput, layout};
    use crate::ui::interactive::{Activity, EditorState, FooterState};
    use unicode_width::UnicodeWidthStr;

    #[test]
    fn empty_editor_has_one_line_and_fixed_chrome() {
        let default_editor = EditorState::default();
        let default_footer = FooterState::default();
        let layout = layout(LayoutInput {
            editor: &default_editor,
            footer: &default_footer,
            queued_messages: 0,
            terminal_width: 8,
        });

        assert_eq!(layout.top_divider, "────────");
        assert_eq!(layout.editor_lines, [""]);
        assert_eq!(layout.footer, "idle");
        assert_eq!(layout.cursor, CursorPosition { row: 0, column: 0 });
        assert_eq!(layout.height(), 4);
    }

    #[test]
    fn explicit_newlines_grow_the_editor() {
        let mut editor = EditorState::default();
        editor.set_text("one\ntwo\n");
        let default_footer = FooterState::default();
        let layout = layout(LayoutInput {
            editor: &editor,
            footer: &default_footer,
            queued_messages: 0,
            terminal_width: 20,
        });

        assert_eq!(layout.editor_lines, ["one", "two", ""]);
        assert_eq!(layout.cursor, CursorPosition { row: 2, column: 0 });
        assert_eq!(layout.height(), 6);
    }

    #[test]
    fn soft_wrap_uses_display_width_for_wide_unicode() {
        let mut editor = EditorState::default();
        editor.set_text("ab界c");
        let default_footer = FooterState::default();
        let layout = layout(LayoutInput {
            editor: &editor,
            footer: &default_footer,
            queued_messages: 0,
            terminal_width: 4,
        });

        assert_eq!(layout.editor_lines, ["ab界", "c"]);
        assert_eq!(layout.cursor, CursorPosition { row: 1, column: 1 });
    }

    #[test]
    fn cursor_tracks_insertion_position_across_wrapped_lines() {
        let mut editor = EditorState::default();
        editor.set_text("abcdef");
        editor.move_left();
        editor.move_left();
        let default_footer = FooterState::default();
        let layout = layout(LayoutInput {
            editor: &editor,
            footer: &default_footer,
            queued_messages: 0,
            terminal_width: 3,
        });

        assert_eq!(layout.editor_lines, ["abc", "def"]);
        assert_eq!(layout.cursor, CursorPosition { row: 1, column: 1 });
    }

    #[test]
    fn full_final_line_adds_a_cursor_line() {
        let mut editor = EditorState::default();
        editor.set_text("界");
        let default_footer = FooterState::default();
        let layout = layout(LayoutInput {
            editor: &editor,
            footer: &default_footer,
            queued_messages: 0,
            terminal_width: 2,
        });

        assert_eq!(layout.editor_lines, ["界", ""]);
        assert_eq!(layout.cursor, CursorPosition { row: 1, column: 0 });
    }

    #[test]
    fn footer_contains_available_status_and_queue_count() {
        let default_editor = EditorState::default();
        let footer = FooterState {
            activity: Activity::Thinking,
            model: "model".into(),
            context: Some("42% context".into()),
            quota: Some("80% quota".into()),
        };
        let layout = layout(LayoutInput {
            editor: &default_editor,
            footer: &footer,
            queued_messages: 2,
            terminal_width: 80,
        });

        assert_eq!(layout.footer, "thinking | model | 42% context | 80% quota | 2 queued");
    }

    #[test]
    fn narrow_layout_never_exceeds_terminal_width() {
        let default_editor = EditorState::default();
        let footer = FooterState {
            activity: Activity::Tool("界tool".into()),
            model: "model".into(),
            context: None,
            quota: None,
        };
        let layout = layout(LayoutInput {
            editor: &default_editor,
            footer: &footer,
            queued_messages: 1,
            terminal_width: 5,
        });

        assert!(layout.footer.width() <= 5);
        assert_eq!(layout.top_divider.width(), 5);
    }
}
