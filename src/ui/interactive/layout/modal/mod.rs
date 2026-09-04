mod options;

use super::text::{visible_width, wrap_to_width};
use crate::ui::interactive::{CursorPosition, ModalMode, ModalState};
use options::{modal_hint, render_modal_options};

pub(crate) fn render_modal_overlay(
    modal: &ModalState,
    width: usize,
    height: usize,
) -> (Vec<String>, CursorPosition, bool) {
    let width = width.max(20);
    let inner_width = width.saturating_sub(4).max(1);
    let budget = height.max(6);

    let mut header_lines = Vec::new();
    header_lines.push("─".repeat(width));
    header_lines.push(format!("  \x1b[1;36m{}\x1b[0m", modal.title.trim()));

    let mut cursor = CursorPosition { row: 0, column: 0 };
    let mut cursor_visible = false;

    if modal.is_searchable {
        let prefix = "  \x1b[1m>\x1b[0m ";
        let filter = &modal.filter_query;
        header_lines.push(format!("{prefix}{filter}"));
        cursor = CursorPosition {
            row: header_lines.len() - 1,
            column: (visible_width(prefix) + visible_width(filter)).min(width),
        };
        cursor_visible = true;
    }

    let input_wrapped = match &modal.mode {
        ModalMode::Input { prompt_label } => {
            let prefix = format!("\x1b[1;36m{prompt_label}:\x1b[0m ");
            let input_text = modal.input.text();
            let prompt_line = format!("{prefix}{input_text}");
            wrap_to_width(&prompt_line, inner_width)
                .into_iter()
                .map(|line| format!("  {line}"))
                .collect::<Vec<_>>()
        }
        ModalMode::Select => Vec::new(),
    };

    let sep_hint = budget >= 8;
    let sep_input = !input_wrapped.is_empty() && budget >= 7;
    let sep_options = budget >= 7;

    let fixed_bottom_count =
        1 + 1 + usize::from(sep_hint) + input_wrapped.len() + usize::from(sep_input) + usize::from(sep_options);

    let reserved_chrome = header_lines.len() + fixed_bottom_count;
    let remaining_for_content = budget.saturating_sub(reserved_chrome);

    let max_opts = if modal.body.trim().is_empty() {
        remaining_for_content.clamp(1, 10)
    } else {
        remaining_for_content.clamp(1, modal.options.len().clamp(1, 5))
    };

    let options_lines = render_modal_options(modal, inner_width, max_opts);
    let space_after_options = remaining_for_content.saturating_sub(options_lines.len());

    let body_lines = render_modal_body(&modal.body, inner_width, space_after_options);

    let mut lines = Vec::new();
    lines.extend(header_lines);
    if let Some(body) = body_lines {
        lines.extend(body);
    }
    if sep_options {
        lines.push(String::new());
    }
    lines.extend(options_lines);

    if let ModalMode::Input { prompt_label } = &modal.mode {
        if sep_input {
            lines.push(String::new());
        }
        let prefix = format!("\x1b[1;36m{prompt_label}:\x1b[0m ");
        let input_text = modal.input.text();
        cursor = CursorPosition {
            row: lines.len(),
            column: (2 + visible_width(&prefix) + visible_width(&input_text[..modal.input.cursor()])).min(width),
        };
        cursor_visible = true;
        lines.extend(input_wrapped);
    }

    if sep_hint {
        lines.push(String::new());
    }
    lines.push(format!("  {}", modal_hint(modal)));
    lines.push("─".repeat(width));

    (lines, cursor, cursor_visible)
}

fn render_modal_body(body: &str, inner_width: usize, space: usize) -> Option<Vec<String>> {
    if body.trim().is_empty() || space < 2 {
        return None;
    }
    let body_budget = space - 1;
    let wrapped = wrap_to_width(body, inner_width);
    let total = wrapped.len();
    if total == 0 {
        return None;
    }

    let mut lines = Vec::new();
    lines.push(String::new());

    if total <= body_budget {
        for line in wrapped {
            lines.push(format!("  {line}"));
        }
    } else if body_budget == 1 {
        lines.push(format_omission_line(total));
    } else {
        let visible = body_budget - 1;
        let omitted = total - visible;
        for line in wrapped.into_iter().take(visible) {
            lines.push(format!("  {line}"));
        }
        lines.push(format_omission_line(omitted));
    }

    Some(lines)
}

fn format_omission_line(omitted: usize) -> String {
    format!("  \x1b[2m[... {omitted} lines omitted ...]\x1b[0m")
}
