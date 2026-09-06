use super::options::{ModalOptionsLayout, render_modal_options};
use crate::ui::interactive::layout::editor::{window_editor, wrap_editor};
use crate::ui::interactive::layout::text::{truncate_to_width, visible_width, wrap_to_width};
use crate::ui::interactive::{CursorPosition, ModalMode, ModalState, OptionLayout};

fn options_desired_lines(modal: &ModalState) -> usize {
    if matches!(modal.mode, ModalMode::Input { .. }) {
        0
    } else if modal.options.is_empty() {
        usize::from(modal.is_searchable)
    } else if modal.option_layout == OptionLayout::Horizontal {
        1
    } else {
        modal.options.len().min(12)
    }
}

pub fn modal_body_max_scroll(modal: &ModalState, draft_text: &str, (width, height): (usize, usize)) -> usize {
    let inner_width = width.saturating_sub(4).max(1);
    let total = wrap_to_width(&modal.body, inner_width).len();
    if total == 0 {
        return 0;
    }
    let desired = in_input_modal_desired_lines(modal, draft_text, inner_width);
    let budget = crate::ui::interactive::layout::budget::compute_normal_budget(
        &crate::ui::interactive::layout::budget::NormalBudgetInput {
            terminal_height: height,
            raw_queued_count: 0,
            raw_widgets_count: 0,
            raw_footer_count: 1,
            total_editor_lines: desired,
            autocomplete_desired: 0,
            is_modal: true,
        },
    );
    let (b_space, _, _, _) = modal_in_input_spaces(modal, draft_text, (budget.editor_max_lines, inner_width));
    total.saturating_sub(b_space.saturating_sub(1))
}

pub fn in_input_modal_desired_lines(modal: &ModalState, draft_text: &str, inner_width: usize) -> usize {
    let search = usize::from(modal.is_searchable);
    let input = if matches!(modal.mode, ModalMode::Input { .. }) {
        wrap_to_width(modal.input.text(), inner_width).len().max(1)
    } else {
        0
    };
    let draft = usize::from(!draft_text.trim().is_empty());
    let body = if modal.body.trim().is_empty() {
        0
    } else {
        wrap_to_width(&modal.body, inner_width).len()
    };
    let options = options_desired_lines(modal);
    (search + input + draft + body + options).max(1)
}

pub struct InInputModalInput<'a> {
    pub modal: &'a ModalState,
    pub draft_text: &'a str,
    pub bounds: (usize, usize),
    pub theme: &'a crate::ui::theme::Theme,
}

pub(crate) fn calculate_content_space(modal: &ModalState, space_for_content: usize) -> (usize, usize) {
    if matches!(modal.mode, ModalMode::Input { .. }) || modal.options.is_empty() {
        (space_for_content, 0)
    } else if modal.option_layout == OptionLayout::Horizontal {
        let opt_space = 1.min(space_for_content);
        (space_for_content.saturating_sub(opt_space), opt_space)
    } else if modal.body.trim().is_empty() || space_for_content <= 2 {
        (0, space_for_content)
    } else {
        let opt_desired = modal
            .options
            .len()
            .min(5)
            .min(space_for_content.saturating_sub(1))
            .max(1);
        (space_for_content.saturating_sub(opt_desired), opt_desired)
    }
}

fn push_search_row(modal: &ModalState, width: usize, lines: &mut Vec<String>) -> (CursorPosition, bool) {
    let cursor = CursorPosition {
        row: lines.len(),
        column: (visible_width("  > ") + visible_width(&modal.filter_query)).min(width),
    };
    lines.push(format!("  \x1b[1m>\x1b[0m {}", modal.filter_query));
    (cursor, true)
}

fn push_modal_input_prompt(
    modal: &ModalState,
    theme: &crate::ui::theme::Theme,
    (width, max_input_lines, lines): (usize, usize, &mut Vec<String>),
) -> Option<(CursorPosition, bool)> {
    let ModalMode::Input { prompt_label } = &modal.mode else {
        return None;
    };
    let highlight = theme.highlight;
    let bold = anstyle::Style::new().bold();
    let prefix = format!("  {highlight}{bold}{prompt_label}:{bold:#}{highlight:#} ");
    let prefix_width = visible_width(&format!("  {prompt_label}: "));
    let cont_prefix = " ".repeat(prefix_width);
    let edit_width = width.saturating_sub(prefix_width).max(1);

    let (wrapped, cursor_pos) = wrap_editor(&modal.input, edit_width);
    let (windowed, cur) = window_editor(wrapped, cursor_pos, max_input_lines.max(1));
    let base_row = lines.len();

    for (idx, line) in windowed.into_iter().enumerate() {
        if idx == 0 {
            lines.push(format!("{prefix}{line}"));
        } else {
            lines.push(format!("{cont_prefix}{line}"));
        }
    }
    let cursor = CursorPosition {
        row: base_row + cur.row,
        column: (prefix_width + cur.column).min(width),
    };
    Some((cursor, true))
}

fn format_draft_line(draft_text: &str, inner_width: usize, theme: &crate::ui::theme::Theme) -> String {
    let single_line = draft_text.trim().replace('\n', " ");
    let max_preview = inner_width.saturating_sub(25).max(5);
    let preview = truncate_to_width(&single_line, max_preview);
    let dimmed = theme.dimmed;
    format!("  {dimmed}Draft: \"{preview}\" (restores on close){dimmed:#}")
}

fn collect_modal_content(
    input: &InInputModalInput<'_>,
    (inner_width, body_space, opt_space): (usize, usize, usize),
    has_draft: bool,
) -> Vec<String> {
    let mut lines = render_in_input_body(input.modal, inner_width, body_space);
    if opt_space > 0 {
        lines.extend(render_modal_options(
            input.modal,
            ModalOptionsLayout {
                inner_width,
                max_visible: opt_space,
                theme: input.theme,
            },
        ));
    }
    if has_draft {
        lines.push(format_draft_line(input.draft_text, inner_width, input.theme));
    }
    lines
}

fn modal_in_input_spaces(
    modal: &ModalState,
    draft_text: &str,
    (max_lines, inner_width): (usize, usize),
) -> (usize, usize, usize, bool) {
    let has_draft = !draft_text.trim().is_empty() && max_lines >= 2;
    let has_search = modal.is_searchable && !matches!(modal.mode, ModalMode::Input { .. });
    let is_input = matches!(modal.mode, ModalMode::Input { .. });
    let input_lines = if is_input {
        wrap_to_width(modal.input.text(), inner_width)
            .len()
            .max(1)
            .min(max_lines.saturating_sub(2).max(1))
    } else {
        0
    };
    let fixed = usize::from(has_search) + input_lines + usize::from(has_draft);
    let (b_space, opt_space) = calculate_content_space(modal, max_lines.saturating_sub(fixed));
    (b_space, opt_space, input_lines, has_draft)
}

pub fn render_in_input_modal(input: InInputModalInput<'_>) -> (Vec<String>, CursorPosition, bool) {
    let (width, max_lines) = (input.bounds.0.max(1), input.bounds.1);
    if max_lines == 0 {
        return (Vec::new(), CursorPosition { row: 0, column: 0 }, false);
    }
    let inner_width = width.saturating_sub(4).max(1);
    let (b_space, opt_space, input_lines, has_draft) =
        modal_in_input_spaces(input.modal, input.draft_text, (max_lines, inner_width));

    let mut lines = Vec::new();
    let mut cursor = (CursorPosition { row: 0, column: 0 }, false);
    if input.modal.is_searchable && !matches!(input.modal.mode, ModalMode::Input { .. }) {
        cursor = push_search_row(input.modal, width, &mut lines);
    }
    lines.extend(collect_modal_content(
        &input,
        (inner_width, b_space, opt_space),
        has_draft,
    ));
    if let Some(c) = push_modal_input_prompt(input.modal, input.theme, (width, input_lines, &mut lines)) {
        cursor = c;
    }
    (lines, cursor.0, cursor.1)
}

fn format_scroll_indicator(current_line: usize, total_lines: usize) -> String {
    format!("  \x1b[2m↑/↓ scroll (line {current_line}/{total_lines})\x1b[0m")
}

fn render_horizontal_body_lines(wrapped: Vec<String>, body_scroll: usize, space: usize) -> Vec<String> {
    let total = wrapped.len();
    if space == 1 {
        return vec![format_scroll_indicator(1, total)];
    }
    let visible_count = space - 1;
    let max_scroll = total.saturating_sub(visible_count);
    let scroll = body_scroll.min(max_scroll);
    let mut lines: Vec<String> = wrapped
        .into_iter()
        .skip(scroll)
        .take(visible_count)
        .map(|l| format!("  {l}"))
        .collect();
    lines.push(format_scroll_indicator(scroll + 1, total));
    lines
}

fn render_vertical_omission_lines(wrapped: Vec<String>, space: usize) -> Vec<String> {
    let total = wrapped.len();
    if space == 1 {
        vec![format_omission_line(total)]
    } else {
        let visible = space - 1;
        let omitted = total - visible;
        let mut lines: Vec<String> = wrapped.into_iter().take(visible).map(|l| format!("  {l}")).collect();
        lines.push(format_omission_line(omitted));
        lines
    }
}

fn render_in_input_body(modal: &ModalState, inner_width: usize, space: usize) -> Vec<String> {
    if modal.body.trim().is_empty() || space == 0 {
        return Vec::new();
    }
    let wrapped = wrap_to_width(&modal.body, inner_width);
    let total = wrapped.len();
    if total == 0 {
        return Vec::new();
    }
    if total <= space {
        wrapped.into_iter().map(|line| format!("  {line}")).collect()
    } else if modal.option_layout == OptionLayout::Horizontal {
        render_horizontal_body_lines(wrapped, modal.body_scroll, space)
    } else {
        render_vertical_omission_lines(wrapped, space)
    }
}

fn format_omission_line(omitted: usize) -> String {
    format!("  \x1b[2m[... {omitted} lines omitted ...]\x1b[0m")
}
