use super::autocomplete::render_autocomplete_dropdown;
use super::budget::{NormalBudgetInput, compute_normal_budget};
use super::chrome::{queued_lines_text, system_lines_text, thinking_divider_style, top_divider, working_line_text};
use super::editor::{window_editor, wrap_editor};
use super::types::{InteractiveLayout, LayoutInput};

pub(crate) fn render_normal_layout(input: LayoutInput<'_>) -> InteractiveLayout {
    let width = input.terminal_width.max(1);
    let mut lines = Vec::new();

    let working_line = working_line_text(input.footer, input.spinner_frame, width);
    let widget_lines = input.widget_lines;
    let queued_lines = queued_lines_text(input.queued_messages, width);
    let system_lines = system_lines_text(input.system_message, width);

    let (all_ed_lines, full_cursor) = wrap_editor(input.editor, width);
    let ac_desired = if let Some(ac) = input.autocomplete {
        if ac.visible && !ac.items.is_empty() && width >= 15 {
            ac.items.len().min(super::autocomplete::MAX_VISIBLE_ITEMS)
        } else {
            0
        }
    } else {
        0
    };
    let ft_lines = crate::ui::interactive::footer::format_footer_lines(input.footer, width);

    let budget = compute_normal_budget(&NormalBudgetInput {
        terminal_height: input.terminal_height,
        raw_widgets_count: widget_lines.len(),
        has_working: !working_line.is_empty(),
        raw_system_count: system_lines.len(),
        raw_queued_count: queued_lines.len(),
        total_editor_lines: all_ed_lines.len(),
        autocomplete_desired: ac_desired,
        raw_footer_count: ft_lines.len(),
    });

    let visible_widgets = if widget_lines.len() > budget.widget_count {
        widget_lines[widget_lines.len() - budget.widget_count..].to_vec()
    } else {
        widget_lines.to_vec()
    };
    if !visible_widgets.is_empty() {
        lines.extend(visible_widgets.clone());
    }

    if budget.show_spacer {
        lines.push(String::new());
    }
    let visible_queued = if budget.queued_count > 0 {
        let count = budget.queued_count.min(queued_lines.len());
        queued_lines[..count].to_vec()
    } else {
        Vec::new()
    };
    lines.extend(visible_queued.clone());

    let visible_system = if budget.system_count > 0 {
        let count = budget.system_count.min(system_lines.len());
        system_lines[..count].to_vec()
    } else {
        Vec::new()
    };
    lines.extend(visible_system.clone());

    let visible_working = if budget.working_count > 0 && !working_line.is_empty() {
        lines.push(working_line.clone());
        working_line
    } else {
        String::new()
    };

    let is_bash_mode = input.editor.text().trim_start().starts_with('!');
    let (style, reset) = if is_bash_mode {
        ("\x1b[33m", "\x1b[0m")
    } else {
        thinking_divider_style(input.footer.thinking_level.as_deref())
    };
    let label = if input.footer.show_label {
        concat!("rho ", env!("CARGO_PKG_VERSION"))
    } else {
        ""
    };
    let top_div = top_divider(width, label, (style, reset));
    if budget.show_top_div {
        lines.push(top_div.clone());
    }

    let ac_lines = if let Some(ac) = input.autocomplete {
        render_autocomplete_dropdown(ac, width, budget.autocomplete_max_lines)
    } else {
        Vec::new()
    };

    let unused_ac = budget.autocomplete_max_lines.saturating_sub(ac_lines.len());
    let ed_max = budget.editor_max_lines + unused_ac.min(all_ed_lines.len().saturating_sub(budget.editor_max_lines));
    let (mut ed_lines, ed_cursor) = window_editor(all_ed_lines, full_cursor, ed_max);
    if !ac_lines.is_empty() {
        ed_lines.extend(ac_lines);
    }

    let editor_start_row = lines.len();
    lines.extend(ed_lines.clone());

    let bot_div = format!("{style}{}{reset}", "─".repeat(width));
    if budget.show_bot_div {
        lines.push(bot_div.clone());
    }

    let visible_ft_lines = ft_lines[..ft_lines.len().min(budget.footer_count)].to_vec();
    let footer_style = crate::ui::theme::Theme::default().dimmed;
    for fl in &visible_ft_lines {
        lines.push(format!("{footer_style}{fl}{footer_style:#}"));
    }

    let footer = visible_ft_lines.join("\n");
    let cursor_row = editor_start_row + ed_cursor.row;

    InteractiveLayout {
        lines,
        cursor: ed_cursor,
        cursor_visible: true,
        cursor_row,
        queued_lines: visible_queued,
        widget_lines: visible_widgets,
        system_lines: visible_system,
        working_line: visible_working,
        top_divider: top_div,
        editor_lines: ed_lines,
        bottom_divider: bot_div,
        footer_lines: visible_ft_lines,
        footer,
    }
}
