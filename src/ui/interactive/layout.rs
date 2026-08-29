use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::{Activity, EditorState, FooterState, ModalMode, ModalState};

const SPINNER_FRAMES: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

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
        let mut h = self.editor_lines.len() + 1;
        if !self.top_divider.is_empty() {
            h += 1;
        }
        if !self.bottom_divider.is_empty() {
            h += 1;
        }
        h
    }
}

pub struct LayoutInput<'a> {
    pub editor: &'a EditorState,
    pub modal: Option<&'a ModalState>,
    pub footer: &'a FooterState,
    pub queued_messages: usize,
    pub terminal_width: usize,
    pub spinner_frame: usize,
}

pub fn layout(input: LayoutInput<'_>) -> InteractiveLayout {
    let width = input.terminal_width.max(1);
    let (top_divider, editor_lines, bottom_divider, cursor) = if let Some(modal) = input.modal {
        let (lines, cursor) = render_modal_overlay(modal, width);
        (String::new(), lines, String::new(), cursor)
    } else {
        let (lines, cursor) = wrap_editor(input.editor, width);
        ("─".repeat(width), lines, "─".repeat(width), cursor)
    };

    InteractiveLayout {
        top_divider,
        editor_lines,
        bottom_divider,
        footer: truncate_to_width(
            &footer_text(input.footer, input.queued_messages, input.spinner_frame),
            width,
        ),
        cursor,
    }
}

fn visible_width(content: &str) -> usize {
    let clean = crate::ui::block::ANSI_PATTERN.replace_all(content, "");
    UnicodeWidthStr::width(clean.as_ref())
}

fn wrap_to_width(content: &str, max_width: usize) -> Vec<String> {
    let max_width = max_width.max(1);
    let mut output = Vec::new();
    for line in content.split('\n') {
        if line.is_empty() {
            output.push(String::new());
            continue;
        }
        let mut current = String::new();
        let mut current_width = 0;
        let mut offset = 0;
        let mut active_ansi = String::new();

        while offset < line.len() {
            if line[offset..].starts_with('\x1b')
                && let Some(end) = line[offset..].find('m')
            {
                let seq = &line[offset..=offset + end];
                current.push_str(seq);
                if seq == "\x1b[0m" {
                    active_ansi.clear();
                } else {
                    active_ansi.push_str(seq);
                }
                offset += end + 1;
                continue;
            }
            let Some(c) = line[offset..].chars().next() else {
                break;
            };
            let char_w = UnicodeWidthChar::width(c).unwrap_or(0);
            if current_width > 0 && current_width + char_w > max_width {
                if !active_ansi.is_empty() {
                    current.push_str("\x1b[0m");
                }
                output.push(std::mem::take(&mut current));
                if !active_ansi.is_empty() {
                    current.push_str(&active_ansi);
                }
                current_width = 0;
            }
            current.push(c);
            current_width += char_w;
            offset += c.len_utf8();
        }
        output.push(current);
    }
    if output.is_empty() {
        output.push(String::new());
    }
    output
}

fn render_modal_overlay(modal: &ModalState, width: usize) -> (Vec<String>, CursorPosition) {
    let width = width.max(20);
    let inner_width = width.saturating_sub(4).max(1);

    let mut lines = Vec::new();
    let mut cursor = CursorPosition { row: 0, column: 0 };

    let title = format!(" {} ", modal.title.trim());
    let title_w = visible_width(&title);
    if title_w + 4 <= width {
        let dashes = "─".repeat(width - title_w - 3);
        lines.push(format!(
            "\x1b[2m╭─\x1b[0m\x1b[1;36m{title}\x1b[0m\x1b[2m{dashes}╮\x1b[0m"
        ));
    } else {
        let dashes = "─".repeat(width.saturating_sub(2));
        lines.push(format!("\x1b[2m╭{dashes}╮\x1b[0m"));
    }

    let push_inner = |lines: &mut Vec<String>, content: &str| {
        let vis_w = visible_width(content);
        let pad = inner_width.saturating_sub(vis_w);
        lines.push(format!("\x1b[2m│ \x1b[0m{content}{}\x1b[2m │\x1b[0m", " ".repeat(pad)));
    };

    if !modal.body.trim().is_empty() {
        for line in wrap_to_width(&modal.body, inner_width) {
            push_inner(&mut lines, &line);
        }
        push_inner(&mut lines, "");
    }

    for (i, opt) in modal.options.iter().enumerate() {
        let is_selected = i == modal.selected;
        let prefix = if is_selected { "\x1b[36m▸\x1b[0m " } else { "  " };
        let label_styled = if is_selected {
            format!("\x1b[1m{}\x1b[0m", opt.label)
        } else {
            opt.label.clone()
        };

        let opt_line = if let Some(desc) = &opt.description {
            format!("{prefix}{label_styled}  \x1b[2m{desc}\x1b[0m")
        } else {
            format!("{prefix}{label_styled}")
        };

        if is_selected && modal.mode == ModalMode::Select {
            cursor = CursorPosition {
                row: lines.len(),
                column: 2,
            };
        }

        for wrapped in wrap_to_width(&opt_line, inner_width) {
            push_inner(&mut lines, &wrapped);
        }
    }

    if let ModalMode::Input { prompt_label } = &modal.mode {
        push_inner(&mut lines, "");
        let prompt_prefix = format!("\x1b[1;36m{prompt_label}:\x1b[0m ");
        let input_text = modal.input.text();
        let prompt_line = format!("{prompt_prefix}{input_text}");

        let prefix_w = visible_width(&prompt_prefix);
        let text_cursor_w = visible_width(&input_text[..modal.input.cursor()]);

        cursor = CursorPosition {
            row: lines.len(),
            column: 2 + prefix_w + text_cursor_w,
        };

        for wrapped in wrap_to_width(&prompt_line, inner_width) {
            push_inner(&mut lines, &wrapped);
        }
    }

    push_inner(&mut lines, "");

    let hint = match &modal.mode {
        ModalMode::Select => {
            if modal.title.contains("Permission") || modal.title.contains("Approve") {
                "\x1b[2m↑/↓ select • Enter confirm • Esc deny\x1b[0m"
            } else if modal.allow_custom {
                "\x1b[2m↑/↓ select • Enter confirm • Esc cancel • or type custom\x1b[0m"
            } else {
                "\x1b[2m↑/↓ select • Enter confirm • Esc cancel\x1b[0m"
            }
        }
        ModalMode::Input { .. } => {
            if modal.options.is_empty() {
                "\x1b[2mEnter submit • Esc cancel\x1b[0m"
            } else {
                "\x1b[2mEnter submit • Esc back\x1b[0m"
            }
        }
    };
    push_inner(&mut lines, hint);

    let dashes = "─".repeat(width.saturating_sub(2));
    lines.push(format!("\x1b[2m╰{dashes}╯\x1b[0m"));

    (lines, cursor)
}

fn footer_text(footer: &FooterState, queued_messages: usize, spinner_frame: usize) -> String {
    let activity = match &footer.activity {
        Activity::Idle => footer.activity.label().to_string(),
        Activity::Thinking | Activity::Tool(_) => {
            let spinner = SPINNER_FRAMES[spinner_frame % SPINNER_FRAMES.len()];
            format!("{spinner} {}", footer.activity.label())
        }
    };
    let mut segments = vec![activity];
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
            modal: None,
            footer: &default_footer,
            queued_messages: 0,
            terminal_width: 8,
            spinner_frame: 0,
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
            modal: None,
            footer: &default_footer,
            queued_messages: 0,
            terminal_width: 20,
            spinner_frame: 0,
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
            modal: None,
            footer: &default_footer,
            queued_messages: 0,
            terminal_width: 4,
            spinner_frame: 0,
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
            modal: None,
            footer: &default_footer,
            queued_messages: 0,
            terminal_width: 3,
            spinner_frame: 0,
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
            modal: None,
            footer: &default_footer,
            queued_messages: 0,
            terminal_width: 2,
            spinner_frame: 0,
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
            modal: None,
            footer: &footer,
            queued_messages: 2,
            terminal_width: 80,
            spinner_frame: 0,
        });

        assert_eq!(layout.footer, "⠋ thinking | model | 42% context | 80% quota | 2 queued");
    }

    #[test]
    fn tool_activity_uses_the_requested_spinner_frame() {
        let default_editor = EditorState::default();
        let footer = FooterState {
            activity: Activity::Tool("read src/lib.rs".into()),
            model: "model".into(),
            context: None,
            quota: None,
        };
        let layout = layout(LayoutInput {
            editor: &default_editor,
            modal: None,
            footer: &footer,
            queued_messages: 0,
            terminal_width: 80,
            spinner_frame: 1,
        });

        assert_eq!(layout.footer, "⠙ read src/lib.rs | model");
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
            modal: None,
            footer: &footer,
            queued_messages: 1,
            terminal_width: 5,
            spinner_frame: 1,
        });

        assert!(layout.footer.width() <= 5);
        assert_eq!(layout.top_divider.width(), 5);
    }

    #[test]
    fn modal_layout_renders_rounded_box_overlay() {
        let default_editor = EditorState::default();
        let default_footer = FooterState::default();
        let modal = crate::ui::interactive::ModalState::new(
            "Permission Required",
            "tool   bash\nscope  cargo test",
            vec![
                crate::ui::interactive::ModalOption::from("Allow"),
                crate::ui::interactive::ModalOption::from("Deny with reason"),
            ],
        );
        let layout = layout(LayoutInput {
            editor: &default_editor,
            modal: Some(&modal),
            footer: &default_footer,
            queued_messages: 0,
            terminal_width: 40,
            spinner_frame: 0,
        });

        assert!(layout.top_divider.is_empty());
        assert!(layout.editor_lines.iter().any(|l| l.contains("Permission Required")));
        assert!(layout.editor_lines.iter().any(|l| l.contains("tool   bash")));
        assert!(layout.editor_lines.iter().any(|l| l.contains("Allow")));
        assert_eq!(layout.cursor.column, 2);
    }
}
