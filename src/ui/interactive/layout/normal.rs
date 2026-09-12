use super::autocomplete::render_autocomplete_dropdown;
use super::budget::{NormalBudgetInput, NormalLayoutBudget, compute_normal_budget};
use super::chrome::{
    modal_banner_title, modal_top_divider, queued_lines_text, thinking_divider_style, top_divider, working_line_text,
};
use super::editor::{render_editor_lines, window_editor, wrap_editor};
use super::types::{CursorPosition, InteractiveLayout, LayoutInput};

fn desired_autocomplete_count(input: &LayoutInput<'_>, width: usize) -> usize {
    if input.modal.is_none()
        && let Some(ac) = input.autocomplete
        && ac.visible
        && !ac.items.is_empty()
        && width >= 15
    {
        ac.items.len().min(super::autocomplete::MAX_VISIBLE_ITEMS)
    } else {
        0
    }
}

fn resolve_divider_style(input: &LayoutInput<'_>) -> (&'static str, &'static str) {
    if input.modal.is_some() {
        ("\x1b[1;36m", "\x1b[0m")
    } else if input.editor.text().trim_start().starts_with('!') {
        ("\x1b[33m", "\x1b[0m")
    } else {
        thinking_divider_style(input.footer.thinking_level.as_deref())
    }
}

fn resolve_top_divider(input: &LayoutInput<'_>, width: usize, style: &str, reset: &str) -> String {
    match input.modal {
        Some(modal) => modal_top_divider(width, modal_banner_title(modal), style, reset),
        None => {
            let label = if input.footer.show_label {
                concat!("rho ", env!("CARGO_PKG_VERSION"))
            } else {
                ""
            };
            let activity = super::chrome::active_activity_status(input.footer, input.spinner_frame);
            top_divider(width, label, activity, style, reset)
        }
    }
}

fn render_editor_area(
    input: &LayoutInput<'_>,
    (all_ed_lines, full_cursor): (Vec<String>, CursorPosition),
    (width, ed_budget, ac_budget, theme): (usize, usize, usize, &crate::ui::theme::Theme),
) -> (Vec<String>, CursorPosition, bool) {
    if let Some(modal) = input.modal {
        return super::modal::render_in_input_modal(super::modal::InInputModalInput {
            modal,
            draft_text: input.editor.text(),
            bounds: (width, ed_budget),
            theme,
            focused: input.focused,
        });
    }
    let ac_lines = input.autocomplete.map_or_else(Vec::new, |ac| {
        render_autocomplete_dropdown(ac, (width, ac_budget), theme)
    });
    let unused_ac = ac_budget.saturating_sub(ac_lines.len());
    let ed_max = ed_budget + unused_ac.min(all_ed_lines.len().saturating_sub(ed_budget));
    let (ed_lines, ed_cursor) = window_editor(all_ed_lines, full_cursor, ed_max);
    let mut ed_lines = if input.focused {
        render_editor_lines(ed_lines, ed_cursor)
    } else {
        ed_lines
    };
    if !ac_lines.is_empty() {
        ed_lines.extend(ac_lines);
    }
    (ed_lines, ed_cursor, input.focused)
}

struct LayoutPieces {
    working: String,
    queued: Vec<String>,
    ed_wrapped: (Vec<String>, CursorPosition),
    ft_lines: Vec<String>,
    budget: NormalLayoutBudget,
}

fn estimate_layout_demands(input: &LayoutInput<'_>, width: usize, ed_len: usize) -> (usize, Vec<String>) {
    let total_ed_lines = input.modal.map_or(ed_len, |m| {
        super::modal::in_input_modal_desired_lines(m, input.editor.text(), width.saturating_sub(4).max(1))
    });
    let ft_lines = input.modal.map_or_else(
        || crate::ui::interactive::footer::format_footer_lines(input.footer, width, input.system_message),
        |m| vec![super::modal::modal_hint(m).to_string()],
    );
    (total_ed_lines, ft_lines)
}

fn prepare_layout_pieces(input: &LayoutInput<'_>, width: usize) -> LayoutPieces {
    let working = working_line_text(input.footer, input.spinner_frame, width);
    let queued = queued_lines_text(input.queued_messages, width);
    let ed_wrapped = wrap_editor(input.editor, width);
    let ac_desired = desired_autocomplete_count(input, width);
    let (total_editor_lines, ft_lines) = estimate_layout_demands(input, width, ed_wrapped.0.len());
    let budget = compute_normal_budget(&NormalBudgetInput {
        terminal_height: input.terminal_height,
        raw_widgets_count: input.widget_lines.len(),
        raw_queued_count: queued.len(),
        total_editor_lines,
        autocomplete_desired: ac_desired,
        raw_footer_count: ft_lines.len(),
        is_modal: input.modal.is_some(),
    });
    LayoutPieces {
        working,
        queued,
        ed_wrapped,
        ft_lines,
        budget,
    }
}

fn window_widget_lines(lines: &[String], budget: usize) -> Vec<String> {
    if lines.len() <= budget {
        return lines.to_vec();
    }
    if budget == 0 {
        return Vec::new();
    }
    let has_borders = lines.first().is_some_and(|l| l.contains('╭')) && lines.last().is_some_and(|l| l.contains('╰'));
    if has_borders && budget >= 3 {
        let top_lines = 2.min(budget.saturating_sub(1));
        let bottom_lines = 1;
        let interior_budget = budget.saturating_sub(top_lines + bottom_lines);
        let interior = &lines[top_lines..lines.len() - bottom_lines];
        let mut result = Vec::with_capacity(budget);
        result.extend_from_slice(&lines[..top_lines]);
        if interior.len() > interior_budget {
            result.extend_from_slice(&interior[interior.len() - interior_budget..]);
        } else {
            result.extend_from_slice(interior);
        }
        result.push(lines.last().unwrap().clone());
        result
    } else if budget >= 2 && !lines.is_empty() {
        let mut result = Vec::with_capacity(budget);
        result.push(lines[0].clone());
        let tail_budget = budget - 1;
        let tail = &lines[1..];
        if tail.len() > tail_budget {
            result.extend_from_slice(&tail[tail.len() - tail_budget..]);
        } else {
            result.extend_from_slice(tail);
        }
        result
    } else {
        lines[lines.len() - budget..].to_vec()
    }
}

fn visible_widgets_and_queued(
    widget_lines: &[String],
    queued_lines: &[String],
    budget: &NormalLayoutBudget,
) -> (Vec<String>, Vec<String>) {
    let vis_widgets = window_widget_lines(widget_lines, budget.widget_count);
    let vis_queued = if budget.queued_count > 0 {
        queued_lines[..budget.queued_count.min(queued_lines.len())].to_vec()
    } else {
        Vec::new()
    };
    (vis_widgets, vis_queued)
}

fn push_pre_editor_lines(
    lines: &mut Vec<String>,
    budget: &NormalLayoutBudget,
    (widgets, queued): (&[String], &[String]),
) {
    if !widgets.is_empty() {
        lines.extend_from_slice(widgets);
    }
    if budget.show_spacer {
        lines.push(String::new());
    }
    if !queued.is_empty() {
        lines.extend_from_slice(queued);
    }
}

fn push_footer_lines(
    lines: &mut Vec<String>,
    (ft_lines, budget_count): (&[String], usize),
    (style, width): (anstyle::Style, usize),
) -> Vec<String> {
    let visible = ft_lines[..ft_lines.len().min(budget_count)].to_vec();
    for fl in &visible {
        let text = super::text::truncate_to_width(fl, width);
        lines.push(format!("{style}{text}{style:#}"));
    }
    visible
}

type AssembleMeta = (
    Vec<String>,
    Vec<String>,
    String,
    String,
    Vec<String>,
    String,
    Vec<String>,
);

fn assemble_layout(
    lines: Vec<String>,
    (cursor, cursor_visible, start_row): (CursorPosition, bool, usize),
    (queued_lines, widget_lines, working_line, top_divider, editor_lines, bottom_divider, footer_lines): AssembleMeta,
) -> InteractiveLayout {
    let footer = footer_lines.join("\n");
    InteractiveLayout {
        lines,
        cursor,
        cursor_visible,
        cursor_row: start_row + cursor.row,
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

fn resolve_chrome_dividers(input: &LayoutInput<'_>, width: usize) -> (String, String) {
    let (style, reset) = resolve_divider_style(input);
    let top = resolve_top_divider(input, width, style, reset);
    let bot = format!("{style}{}{reset}", "─".repeat(width));
    (top, bot)
}

fn init_layout_lines(
    budget: &NormalLayoutBudget,
    (widgets, queued): (&[String], &[String]),
    top_div: &str,
) -> Vec<String> {
    let mut lines = Vec::new();
    push_pre_editor_lines(&mut lines, budget, (widgets, queued));
    if budget.show_top_div {
        lines.push(top_div.to_string());
    }
    lines
}

fn render_editor_and_bottom(
    (input, ed_wrapped): (&LayoutInput<'_>, (Vec<String>, CursorPosition)),
    (width, budget, theme, bot_div): (usize, &NormalLayoutBudget, &crate::ui::theme::Theme, &str),
    lines: &mut Vec<String>,
) -> ((CursorPosition, bool, usize), Vec<String>) {
    let editor_start_row = lines.len();
    let (ed_lines, ed_cursor, ed_vis) = render_editor_area(
        input,
        ed_wrapped,
        (width, budget.editor_max_lines, budget.autocomplete_max_lines, theme),
    );
    lines.extend(ed_lines.clone());
    if budget.show_bot_div {
        lines.push(bot_div.to_string());
    }
    ((ed_cursor, ed_vis, editor_start_row), ed_lines)
}

pub(crate) fn render_normal_layout(input: LayoutInput<'_>) -> InteractiveLayout {
    let width = input.terminal_width.max(1);
    let pieces = prepare_layout_pieces(&input, width);
    let (top_div, bot_div) = resolve_chrome_dividers(&input, width);
    let default_theme = crate::ui::theme::Theme::default();
    let theme = input.theme.unwrap_or(&default_theme);
    let (vis_w, vis_q) = visible_widgets_and_queued(input.widget_lines, &pieces.queued, &pieces.budget);

    let mut lines = init_layout_lines(&pieces.budget, (&vis_w, &vis_q), &top_div);
    let (cursor_info, ed_lines) = render_editor_and_bottom(
        (&input, pieces.ed_wrapped),
        (width, &pieces.budget, theme, &bot_div),
        &mut lines,
    );
    let vis_ft = push_footer_lines(
        &mut lines,
        (&pieces.ft_lines, pieces.budget.footer_count),
        (theme.dimmed, width),
    );

    assemble_layout(
        lines,
        cursor_info,
        (vis_q, vis_w, pieces.working, top_div, ed_lines, bot_div, vis_ft),
    )
}
