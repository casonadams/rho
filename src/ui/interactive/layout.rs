use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::{Activity, EditorState, FooterState, ModalMode, ModalState};

pub const SPINNER_FRAMES: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisualTruncateResult {
    pub visual_lines: Vec<String>,
    pub skipped_count: usize,
}

pub fn truncate_to_visual_lines(text: &str, max_visual_lines: usize, width: usize) -> VisualTruncateResult {
    if text.is_empty() {
        return VisualTruncateResult {
            visual_lines: Vec::new(),
            skipped_count: 0,
        };
    }
    let all_lines = wrap_to_width(text, width.max(1));
    if all_lines.len() <= max_visual_lines {
        return VisualTruncateResult {
            visual_lines: all_lines,
            skipped_count: 0,
        };
    }
    let skipped_count = all_lines.len() - max_visual_lines;
    let visual_lines = all_lines[skipped_count..].to_vec();
    VisualTruncateResult {
        visual_lines,
        skipped_count,
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ActiveToolDisplayInput<'a> {
    pub tool_name: &'a str,
    pub args_summary: &'a str,
    pub preview: Option<&'a str>,
    pub output: &'a str,
    pub started: std::time::Instant,
    pub theme: &'a crate::ui::theme::Theme,
    pub width: usize,
    pub expanded: bool,
}

pub fn format_active_tool_block(input: ActiveToolDisplayInput<'_>) -> String {
    let width = input.width.max(20);
    let title_style = input.theme.tool_header;
    let accent_style = input.theme.highlight;
    let dim_style = input.theme.dimmed;

    let header = format!(
        "{title_style}{}{title_style:#} {accent_style}{}{accent_style:#}",
        input.tool_name, input.args_summary
    );
    let mut content = header;

    if let Some(preview) = input.preview
        && !preview.trim().is_empty()
    {
        content.push('\n');
        content.push_str(preview);
    }

    let clean_output = input.output.trim_end();
    if !clean_output.is_empty() {
        content.push('\n');
        if input.expanded {
            for line in clean_output.lines() {
                content.push('\n');
                content.push_str(&format!("{dim_style}{line}{dim_style:#}"));
            }
        } else {
            let truncated = truncate_to_visual_lines(clean_output, 5, width.saturating_sub(4).max(1));
            if truncated.skipped_count > 0 {
                content.push('\n');
                content.push_str(&format!(
                    "{dim_style}... ({} earlier lines, Ctrl+O to expand){dim_style:#}",
                    truncated.skipped_count
                ));
            }
            for line in truncated.visual_lines {
                content.push('\n');
                content.push_str(&format!("{dim_style}{line}{dim_style:#}"));
            }
        }
    }

    let elapsed = input.started.elapsed();
    let elapsed_text = format!("Elapsed {}", crate::ui::render::format_duration(elapsed));
    content.push('\n');
    content.push_str(&format!("{dim_style}{elapsed_text}{dim_style:#}"));

    crate::ui::block::BlockFormat::new(input.theme.tool_success_bg, width)
        .with_vertical_padding()
        .render_styled(&content)
}

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
    pub footer: String,
    pub cursor: CursorPosition,
}

impl InteractiveLayout {
    pub fn height(&self) -> usize {
        let mut h = self.editor_lines.len() + self.working_line.len().min(1) + self.queued_lines.len() + 1;
        if !self.top_divider.is_empty() {
            h += 1;
        }
        if !self.bottom_divider.is_empty() {
            h += 1;
        }
        h
    }

    pub fn cursor_row(&self) -> usize {
        self.cursor.row
            + self.queued_lines.len()
            + self.working_line.len().min(1)
            + usize::from(!self.top_divider.is_empty())
    }
}

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
    let (top_divider, editor_lines, bottom_divider, cursor, working_line, queued_lines) =
        if let Some(modal) = input.modal {
            let (lines, cursor) = render_modal_overlay(modal, width);
            (String::new(), lines, String::new(), cursor, String::new(), Vec::new())
        } else {
            let (lines, cursor) = wrap_editor(input.editor, width);
            (
                "─".repeat(width),
                lines,
                "─".repeat(width),
                cursor,
                working_line_text(&input.footer.activity, input.spinner_frame, width),
                queued_lines_text(input.queued_messages, width),
            )
        };

    InteractiveLayout {
        queued_lines,
        top_divider,
        editor_lines,
        bottom_divider,
        working_line,
        footer: truncate_to_width(&footer_text(input.footer), width),
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
    if matches!(activity, Activity::Idle) || width < "\u{280b} Working".width().max(1) {
        return String::new();
    }
    let spinner = SPINNER_FRAMES[spinner_frame % SPINNER_FRAMES.len()];
    let accent = "\x1b[36m";
    let reset = "\x1b[0m";
    let dim = "\x1b[2m";
    let full = format!("{accent}{spinner}{reset} {dim}Working...{reset}");
    if full.width() <= width {
        full
    } else {
        // Show the spinner with as much of the label as fits instead of truncating mid-glyph.
        let plain = "\u{280b} Working";
        let shown = truncate_to_width(plain, width.saturating_sub(1));
        let dots = width.saturating_sub(shown.width() + 1).min(3);
        format!("{accent}{spinner}{reset} {dim}{}{}{reset}", shown, ".".repeat(dots))
    }
}

fn visible_width(content: &str) -> usize {
    let clean = crate::ui::block::ANSI_PATTERN.replace_all(content, "");
    UnicodeWidthStr::width(clean.as_ref())
}

pub fn wrap_to_width(content: &str, max_width: usize) -> Vec<String> {
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

    lines.push("─".repeat(width));
    lines.push(format!("  \x1b[1;36m{}\x1b[0m", modal.title.trim()));
    lines.push(String::new());

    if !modal.body.trim().is_empty() {
        for line in wrap_to_width(&modal.body, inner_width) {
            lines.push(format!("  {line}"));
        }
        lines.push(String::new());
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
            lines.push(format!("  {wrapped}"));
        }
    }

    if let ModalMode::Input { prompt_label } = &modal.mode {
        lines.push(String::new());
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
            lines.push(format!("  {wrapped}"));
        }
    }

    lines.push(String::new());

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
    lines.push(format!("  {hint}"));
    lines.push("─".repeat(width));

    (lines, cursor)
}

fn footer_text(footer: &FooterState) -> String {
    let mut segments = Vec::new();
    if !footer.model.is_empty() {
        segments.push(footer.model.clone());
    }
    if let Some(context) = footer.context.as_deref().filter(|value| !value.is_empty()) {
        segments.push(context.to_string());
    }
    if let Some(quota) = footer.quota.as_deref().filter(|value| !value.is_empty()) {
        segments.push(quota.to_string());
    }
    if segments.is_empty() {
        segments.push(footer.activity.label().to_string());
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
            queued_messages: &[],
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
            queued_messages: &[],
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
            queued_messages: &[],
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
            queued_messages: &[],
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
            queued_messages: &[],
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
            queued_messages: &[],
            terminal_width: 80,
            spinner_frame: 0,
        });

        assert_eq!(layout.footer, "model | 42% context | 80% quota");
    }

    #[test]
    fn queued_messages_render_above_the_working_line() {
        let default_editor = EditorState::default();
        let footer = FooterState {
            activity: Activity::Working,
            model: "model".into(),
            context: None,
            quota: None,
        };
        let queued = vec![
            crate::ui::interactive::QueuedMessage {
                text: "first steer".into(),
                kind: crate::ui::interactive::QueueKind::Steering,
            },
            crate::ui::interactive::QueuedMessage {
                text: "next follow".into(),
                kind: crate::ui::interactive::QueueKind::FollowUp,
            },
        ];
        let layout = layout(LayoutInput {
            editor: &default_editor,
            modal: None,
            footer: &footer,
            queued_messages: &queued,
            terminal_width: 80,
            spinner_frame: 0,
        });

        assert_eq!(layout.queued_lines.len(), 3);
        assert!(layout.queued_lines[0].contains("Steering: first steer"));
        assert!(layout.queued_lines[1].contains("Follow-up: next follow"));
        assert!(layout.queued_lines[2].contains("Alt+↑"));
        assert_eq!(layout.height(), 8);
    }

    #[test]
    fn busy_activity_renders_working_line_above_the_editor() {
        let default_editor = EditorState::default();
        let footer = FooterState {
            activity: Activity::Working,
            model: "model".into(),
            context: None,
            quota: None,
        };
        let layout = layout(LayoutInput {
            editor: &default_editor,
            modal: None,
            footer: &footer,
            queued_messages: &[],
            terminal_width: 80,
            spinner_frame: 0,
        });

        assert!(layout.working_line.contains('\u{280b}'));
        assert!(layout.working_line.contains("Working..."));
        assert!(layout.working_line.contains("\u{1b}[2m"));
        assert_eq!(layout.footer, "model");
        assert_eq!(layout.height(), 5);
    }

    #[test]
    fn thinking_activity_also_renders_the_working_line() {
        let default_editor = EditorState::default();
        let footer = FooterState {
            activity: Activity::Thinking,
            model: "model".into(),
            context: None,
            quota: None,
        };
        let layout = layout(LayoutInput {
            editor: &default_editor,
            modal: None,
            footer: &footer,
            queued_messages: &[],
            terminal_width: 80,
            spinner_frame: 0,
        });

        assert!(layout.working_line.contains("Working..."));
    }

    #[test]
    fn idle_activity_renders_no_working_line() {
        let default_editor = EditorState::default();
        let footer = FooterState {
            activity: Activity::Idle,
            model: "model".into(),
            context: None,
            quota: None,
        };
        let layout = layout(LayoutInput {
            editor: &default_editor,
            modal: None,
            footer: &footer,
            queued_messages: &[],
            terminal_width: 80,
            spinner_frame: 0,
        });

        assert_eq!(layout.working_line, "");
        assert_eq!(layout.height(), 4);
    }

    #[test]
    fn busy_activity_under_modal_hides_working_line() {
        let default_editor = EditorState::default();
        let footer = FooterState {
            activity: Activity::Working,
            model: "model".into(),
            context: None,
            quota: None,
        };
        let modal = crate::ui::interactive::ModalState::new(
            "Permission Required",
            "tool   bash\nscope  cargo test",
            vec![crate::ui::interactive::ModalOption::from("Allow")],
        );
        let layout = layout(LayoutInput {
            editor: &default_editor,
            modal: Some(&modal),
            footer: &footer,
            queued_messages: &[],
            terminal_width: 80,
            spinner_frame: 0,
        });

        assert_eq!(layout.working_line, "");
    }

    #[test]
    fn editor_layout_tracks_lines_and_dividers() {
        let mut editor = EditorState::default();
        editor.set_text("draft");
        let default_footer = FooterState::default();
        let layout = layout(LayoutInput {
            editor: &editor,
            modal: None,
            footer: &default_footer,
            queued_messages: &[],
            terminal_width: 80,
            spinner_frame: 0,
        });

        assert_eq!(layout.editor_lines.len(), 1);
        assert_eq!(layout.height(), 4);
        assert_eq!(layout.cursor_row(), 1);
    }

    #[test]
    fn multiline_editor_height_matches_content() {
        let mut editor = EditorState::default();
        editor.set_text("line1\nline2\nline3");
        let default_footer = FooterState::default();
        let layout = layout(LayoutInput {
            editor: &editor,
            modal: None,
            footer: &default_footer,
            queued_messages: &[],
            terminal_width: 80,
            spinner_frame: 0,
        });

        assert_eq!(layout.editor_lines.len(), 3);
        assert_eq!(layout.height(), 6);
    }

    #[test]
    fn narrow_layout_never_exceeds_terminal_width() {
        let default_editor = EditorState::default();
        let footer = FooterState {
            activity: Activity::Working,
            model: "model".into(),
            context: None,
            quota: None,
        };
        let layout = layout(LayoutInput {
            editor: &default_editor,
            modal: None,
            footer: &footer,
            queued_messages: &[],
            terminal_width: 5,
            spinner_frame: 1,
        });

        assert!(layout.footer.width() <= 5);
        assert_eq!(layout.top_divider.width(), 5);
    }

    #[test]
    fn modal_layout_renders_input_frame_style() {
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
            queued_messages: &[],
            terminal_width: 40,
            spinner_frame: 0,
        });

        assert!(layout.top_divider.is_empty());
        assert!(layout.editor_lines.iter().any(|l| l.contains("─".repeat(40).as_str())));
        assert!(layout.editor_lines.iter().any(|l| l.contains("Permission Required")));
        assert!(layout.editor_lines.iter().any(|l| l.contains("tool   bash")));
        assert!(layout.editor_lines.iter().any(|l| l.contains("Allow")));
        assert_eq!(layout.cursor.column, 2);
    }

    #[test]
    fn truncate_to_visual_lines_preserves_short_content() {
        let text = "line1\nline2\nline3";
        let res = super::truncate_to_visual_lines(text, 5, 40);
        assert_eq!(res.visual_lines, ["line1", "line2", "line3"]);
        assert_eq!(res.skipped_count, 0);
    }

    #[test]
    fn truncate_to_visual_lines_skips_earlier_lines_when_exceeding_limit() {
        let text = "line1\nline2\nline3\nline4\nline5\nline6\nline7";
        let res = super::truncate_to_visual_lines(text, 5, 40);
        assert_eq!(res.visual_lines, ["line3", "line4", "line5", "line6", "line7"]);
        assert_eq!(res.skipped_count, 2);
    }

    #[test]
    fn format_active_tool_block_contains_command_and_elapsed() {
        let theme = crate::ui::theme::Theme::default();
        let formatted = super::format_active_tool_block(super::ActiveToolDisplayInput {
            tool_name: "bash",
            args_summary: "cargo test",
            preview: None,
            output: "compiling...\ntest result: ok",
            started: std::time::Instant::now(),
            theme: &theme,
            width: 60,
            expanded: false,
        });

        assert!(formatted.contains("bash"));
        assert!(formatted.contains("cargo test"));
        assert!(formatted.contains("test result: ok"));
        assert!(formatted.contains("Elapsed"));
    }

    #[test]
    fn format_active_tool_block_renders_diff_preview_for_edit() {
        let theme = crate::ui::theme::Theme::default();
        let diff_preview = "```diff\n- old text\n+ new text\n```";
        let formatted = super::format_active_tool_block(super::ActiveToolDisplayInput {
            tool_name: "edit",
            args_summary: "src/main.rs (1 edits)",
            preview: Some(diff_preview),
            output: "",
            started: std::time::Instant::now(),
            theme: &theme,
            width: 60,
            expanded: false,
        });

        assert!(formatted.contains("edit"));
        assert!(formatted.contains("src/main.rs"));
        assert!(formatted.contains("- old text"));
        assert!(formatted.contains("+ new text"));
        assert!(formatted.contains("Elapsed"));
    }

    #[test]
    fn format_active_tool_block_includes_expand_hint_when_truncated() {
        let theme = crate::ui::theme::Theme::default();
        let output = "1\n2\n3\n4\n5\n6\n7\n8\n9\n10";
        let formatted = super::format_active_tool_block(super::ActiveToolDisplayInput {
            tool_name: "bash",
            args_summary: "cargo test",
            preview: None,
            output,
            started: std::time::Instant::now(),
            theme: &theme,
            width: 60,
            expanded: false,
        });

        assert!(formatted.contains("earlier lines"));
        assert!(formatted.contains("Ctrl+O to expand"));
    }
}
