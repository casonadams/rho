pub const MAX_MODAL_HEIGHT_RATIO: f64 = 0.66;
pub const MAX_WIDGET_HEIGHT_RATIO: f64 = 0.60;

pub(crate) struct NormalBudgetInput {
    pub terminal_height: usize,
    pub raw_queued_count: usize,
    pub raw_widgets_count: usize,
    pub raw_footer_count: usize,
    pub total_editor_lines: usize,
    pub autocomplete_desired: usize,
    pub is_modal: bool,
    pub has_activity: bool,
}

pub(crate) struct NormalLayoutBudget {
    pub show_spacer: bool,
    pub show_activity_row: bool,
    pub show_top_div: bool,
    pub show_bot_div: bool,
    pub footer_count: usize,
    pub widget_count: usize,
    pub queued_count: usize,
    pub editor_max_lines: usize,
    pub autocomplete_max_lines: usize,
}

#[derive(Debug, Clone, Copy)]
struct ChromeVisibility {
    show_spacer: bool,
    show_activity_row: bool,
    show_top_div: bool,
    show_bot_div: bool,
    footer_count: usize,
}

fn compute_chrome(budget: usize, raw_footer_count: usize) -> ChromeVisibility {
    let (s, a, t, b, f) = match budget {
        0..=1 => (false, false, false, false, 0),
        2 => (false, false, true, false, 0),
        3 => (false, false, true, true, 0),
        4 => (false, false, true, true, raw_footer_count.min(1)),
        5 => (false, false, true, true, raw_footer_count.min(2)),
        6 => (false, true, true, true, raw_footer_count.min(2)),
        _ => (true, true, true, true, raw_footer_count.min(2)),
    };
    ChromeVisibility {
        show_spacer: s,
        show_activity_row: a,
        show_top_div: t,
        show_bot_div: b,
        footer_count: f,
    }
}

fn allocate_widgets(
    raw_widgets: usize,
    surplus: &mut usize,
    extra_ed: usize,
    ac_desired: usize,
    terminal_height: usize,
) -> usize {
    if raw_widgets == 0 {
        return 0;
    }
    let max_widget = ((terminal_height as f64) * MAX_WIDGET_HEIGHT_RATIO).round() as usize;
    let bounded_raw = raw_widgets.min(max_widget.max(8));
    let ac_min = if ac_desired >= 2 { 2 } else { 0 };
    let needed = extra_ed + ac_min;
    let grant = if *surplus >= needed {
        bounded_raw.min(*surplus - needed)
    } else {
        (*surplus / 3).min(bounded_raw)
    };
    *surplus -= grant;
    grant
}

fn allocate_editor_and_autocomplete(surplus: usize, extra_ed: usize, ac_desired: usize) -> (usize, usize) {
    if ac_desired >= 2 && surplus >= 2 {
        if extra_ed == 0 {
            (ac_desired.min(surplus), 1)
        } else {
            let half = surplus / 2;
            let ac_grant = ac_desired.min(half.max(2)).min(surplus);
            let ed_grant = 1 + surplus.saturating_sub(ac_grant).min(extra_ed);
            (ac_grant, ed_grant)
        }
    } else {
        (0, 1 + surplus.min(extra_ed))
    }
}

fn calculate_surplus(budget: usize, chrome: ChromeVisibility, queued_raw: usize) -> (usize, usize) {
    let reserved = usize::from(chrome.show_spacer)
        + usize::from(chrome.show_activity_row)
        + usize::from(chrome.show_top_div)
        + usize::from(chrome.show_bot_div)
        + chrome.footer_count
        + 1;
    let surplus = budget.saturating_sub(reserved);
    let queued = queued_raw.min(surplus);
    (surplus - queued, queued)
}

fn resolve_chrome(input: &NormalBudgetInput, mut chrome: ChromeVisibility) -> ChromeVisibility {
    if input.is_modal {
        chrome.show_spacer = (input.raw_widgets_count > 0 || input.raw_queued_count > 0) && chrome.show_spacer;
        chrome.show_activity_row = input.has_activity && chrome.show_activity_row;
    }
    chrome
}

pub(crate) fn compute_normal_budget(input: &NormalBudgetInput) -> NormalLayoutBudget {
    let budget = input.terminal_height.max(1);
    let chrome = resolve_chrome(input, compute_chrome(budget, input.raw_footer_count));
    let (mut surplus, queued_count) = calculate_surplus(budget, chrome, input.raw_queued_count);
    let extra_ed = input.total_editor_lines.saturating_sub(1);
    let ac_desired = input.autocomplete_desired;
    let widget_count = allocate_widgets(
        input.raw_widgets_count,
        &mut surplus,
        extra_ed,
        ac_desired,
        input.terminal_height,
    );
    let (autocomplete_max_lines, mut editor_max_lines) =
        allocate_editor_and_autocomplete(surplus, extra_ed, ac_desired);
    if input.is_modal {
        let max_modal = ((input.terminal_height as f64) * MAX_MODAL_HEIGHT_RATIO).round() as usize;
        editor_max_lines = editor_max_lines.min(max_modal.max(1));
    }
    NormalLayoutBudget {
        show_spacer: chrome.show_spacer,
        show_activity_row: chrome.show_activity_row,
        show_top_div: chrome.show_top_div,
        show_bot_div: chrome.show_bot_div,
        footer_count: chrome.footer_count,
        widget_count,
        queued_count,
        editor_max_lines,
        autocomplete_max_lines,
    }
}
