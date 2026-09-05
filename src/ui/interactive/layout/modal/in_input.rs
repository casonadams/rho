use super::options::{ModalOptionsLayout, render_modal_options};
use crate::ui::interactive::layout::text::{truncate_to_width, visible_width, wrap_to_width};
use crate::ui::interactive::{CursorPosition, ModalMode, ModalState};

pub fn in_input_modal_desired_lines(modal: &ModalState, draft_text: &str, inner_width: usize) -> usize {
    let search = usize::from(modal.is_searchable);
    let input = usize::from(matches!(modal.mode, ModalMode::Input { .. }));
    let draft = usize::from(!draft_text.trim().is_empty());
    let body = if modal.body.trim().is_empty() {
        0
    } else {
        wrap_to_width(&modal.body, inner_width).len().min(8)
    };
    let options = if matches!(modal.mode, ModalMode::Input { .. }) {
        0
    } else if modal.options.is_empty() {
        usize::from(modal.is_searchable)
    } else {
        modal.options.len().min(12)
    };
    (search + input + draft + body + options).max(1)
}

pub struct InInputModalInput<'a> {
    pub modal: &'a ModalState,
    pub draft_text: &'a str,
    pub bounds: (usize, usize),
    pub theme: &'a crate::ui::theme::Theme,
}

pub fn render_in_input_modal(input: InInputModalInput<'_>) -> (Vec<String>, CursorPosition, bool) {
    let InInputModalInput {
        modal,
        draft_text,
        bounds,
        theme,
    } = input;
    let (width, max_lines) = bounds;
    let width = width.max(1);
    let inner_width = width.saturating_sub(4).max(1);
    if max_lines == 0 {
        return (Vec::new(), CursorPosition { row: 0, column: 0 }, false);
    }

    let is_input_mode = matches!(modal.mode, ModalMode::Input { .. });
    let has_search = modal.is_searchable && !is_input_mode;
    let has_draft = !draft_text.trim().is_empty() && max_lines >= 2;

    let fixed_count = usize::from(has_search) + usize::from(is_input_mode) + usize::from(has_draft);
    let space_for_content = max_lines.saturating_sub(fixed_count);

    let (body_space, options_space) = if is_input_mode {
        (space_for_content, 0)
    } else if modal.body.trim().is_empty() {
        (0, space_for_content)
    } else if modal.options.is_empty() {
        (space_for_content, 0)
    } else if space_for_content <= 2 {
        (0, space_for_content)
    } else {
        let opt_desired = modal
            .options
            .len()
            .min(5)
            .min(space_for_content.saturating_sub(1))
            .max(1);
        let b_space = space_for_content.saturating_sub(opt_desired);
        (b_space, opt_desired)
    };

    let body_lines = render_in_input_body(&modal.body, inner_width, body_space);

    let options_lines = if options_space > 0 {
        render_modal_options(
            modal,
            ModalOptionsLayout {
                inner_width,
                max_visible: options_space,
                theme,
            },
        )
    } else {
        Vec::new()
    };

    let mut lines = Vec::new();
    let mut cursor = CursorPosition { row: 0, column: 0 };
    let mut cursor_visible = false;

    if has_search {
        let search_prefix = "  \x1b[1m>\x1b[0m ";
        cursor = CursorPosition {
            row: lines.len(),
            column: (visible_width("  > ") + visible_width(&modal.filter_query)).min(width),
        };
        cursor_visible = true;
        lines.push(format!("{search_prefix}{}", modal.filter_query));
    }

    lines.extend(body_lines);
    lines.extend(options_lines);

    if let ModalMode::Input { prompt_label } = &modal.mode {
        let highlight = theme.highlight;
        let bold = anstyle::Style::new().bold();
        let prefix = format!("  {highlight}{bold}{prompt_label}:{bold:#}{highlight:#} ");
        let input_text = modal.input.text();
        let cursor_byte = modal.input.cursor().min(input_text.len());
        let col =
            (visible_width(&format!("  {prompt_label}: ")) + visible_width(&input_text[..cursor_byte])).min(width);
        cursor = CursorPosition {
            row: lines.len(),
            column: col,
        };
        cursor_visible = true;
        lines.push(format!("{prefix}{input_text}"));
    }

    if has_draft {
        let single_line = draft_text.trim().replace('\n', " ");
        let max_preview = inner_width.saturating_sub(25).max(5);
        let preview = truncate_to_width(&single_line, max_preview);
        let dimmed = theme.dimmed;
        lines.push(format!("  {dimmed}Draft: \"{preview}\" (restores on close){dimmed:#}"));
    }

    (lines, cursor, cursor_visible)
}

fn render_in_input_body(body: &str, inner_width: usize, space: usize) -> Vec<String> {
    if body.trim().is_empty() || space == 0 {
        return Vec::new();
    }
    let wrapped = wrap_to_width(body, inner_width);
    let total = wrapped.len();
    if total == 0 {
        return Vec::new();
    }
    if total <= space {
        wrapped.into_iter().map(|line| format!("  {line}")).collect()
    } else if space == 1 {
        vec![format_omission_line(total)]
    } else {
        let visible = space - 1;
        let omitted = total - visible;
        let mut lines: Vec<String> = wrapped.into_iter().take(visible).map(|l| format!("  {l}")).collect();
        lines.push(format_omission_line(omitted));
        lines
    }
}

fn format_omission_line(omitted: usize) -> String {
    format!("  \x1b[2m[... {omitted} lines omitted ...]\x1b[0m")
}
