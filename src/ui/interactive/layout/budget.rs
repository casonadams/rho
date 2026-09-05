pub(crate) struct NormalBudgetInput {
    pub terminal_height: usize,
    pub raw_widgets_count: usize,
    pub raw_queued_count: usize,
    pub total_editor_lines: usize,
    pub autocomplete_desired: usize,
    pub raw_footer_count: usize,
}

pub(crate) struct NormalLayoutBudget {
    pub show_activity_row: bool,
    pub show_top_div: bool,
    pub show_bot_div: bool,
    pub footer_count: usize,
    pub widget_count: usize,
    pub queued_count: usize,
    pub editor_max_lines: usize,
    pub autocomplete_max_lines: usize,
}

pub(crate) fn compute_normal_budget(input: &NormalBudgetInput) -> NormalLayoutBudget {
    let budget = input.terminal_height.max(1);

    let (show_activity_row, show_top_div, show_bot_div, footer_count) = match budget {
        0..=1 => (false, false, false, 0),
        2 => (false, true, false, 0),
        3 => (false, true, true, 0),
        4 => (false, true, true, input.raw_footer_count.min(1)),
        5 => (false, true, true, input.raw_footer_count.min(2)),
        _ => (true, true, true, input.raw_footer_count.min(2)),
    };

    let min_editor = 1;
    let reserved_chrome = usize::from(show_activity_row)
        + usize::from(show_top_div)
        + usize::from(show_bot_div)
        + footer_count
        + min_editor;

    let mut surplus = budget.saturating_sub(reserved_chrome);

    let queued_count = input.raw_queued_count.min(surplus);
    surplus -= queued_count;

    let extra_ed_desired = input.total_editor_lines.saturating_sub(1);
    let ac_desired = input.autocomplete_desired;

    let widget_count = if input.raw_widgets_count > 0 {
        let needed_by_input = extra_ed_desired + if ac_desired >= 2 { 2 } else { 0 };
        if surplus >= needed_by_input {
            let available_for_widgets = surplus - needed_by_input;
            let grant = input.raw_widgets_count.min(available_for_widgets);
            surplus -= grant;
            grant
        } else {
            let max_w = (surplus / 3).min(input.raw_widgets_count);
            surplus -= max_w;
            max_w
        }
    } else {
        0
    };

    let (autocomplete_max_lines, editor_max_lines) = if ac_desired >= 2 && surplus >= 2 {
        if extra_ed_desired == 0 {
            let ac_grant = ac_desired.min(surplus);
            (ac_grant, 1)
        } else {
            let half = surplus / 2;
            let ac_grant = ac_desired.min(half.max(2)).min(surplus);
            let ed_grant = 1 + surplus.saturating_sub(ac_grant).min(extra_ed_desired);
            (ac_grant, ed_grant)
        }
    } else {
        let ed_grant = 1 + surplus.min(extra_ed_desired);
        (0, ed_grant)
    };

    NormalLayoutBudget {
        show_activity_row,
        show_top_div,
        show_bot_div,
        footer_count,
        widget_count,
        queued_count,
        editor_max_lines,
        autocomplete_max_lines,
    }
}
