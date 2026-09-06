use super::options::ModalOptionsLayout;
use crate::ui::interactive::layout::text::visible_width;
use crate::ui::interactive::{ModalOption, ModalState};

fn horizontal_item_width(opt: &ModalOption, is_selected: bool) -> usize {
    visible_width(&opt.label) + if is_selected { 2 } else { 0 }
}

fn window_width(modal: &ModalState, start: usize, end: usize) -> usize {
    let mut width = 0;
    if start > 0 {
        width += 2;
    }
    for i in start..=end {
        width += horizontal_item_width(&modal.options[i], i == modal.selected);
        if i < end {
            width += 3;
        }
    }
    if end + 1 < modal.options.len() {
        width += 2;
    }
    width
}

fn expand_window(modal: &ModalState, inner_width: usize, (mut start, mut end): (usize, usize)) -> (usize, usize) {
    let (n, sel) = (
        modal.options.len(),
        modal.selected.min(modal.options.len().saturating_sub(1)),
    );
    loop {
        let can_left = start > 0 && window_width(modal, start - 1, end) <= inner_width;
        let can_right = end + 1 < n && window_width(modal, start, end + 1) <= inner_width;
        if !can_left && !can_right {
            break;
        }
        if can_left && can_right {
            if sel - start <= end - sel {
                start -= 1;
            } else {
                end += 1;
            }
        } else if can_left {
            start -= 1;
        } else {
            end += 1;
        }
    }
    (start, end)
}

pub(super) fn calculate_horizontal_window(modal: &ModalState, inner_width: usize) -> (usize, usize) {
    let n = modal.options.len();
    if n == 0 {
        return (0, 0);
    }
    let sel = modal.selected.min(n - 1);
    if window_width(modal, 0, n - 1) <= inner_width {
        return (0, n - 1);
    }
    expand_window(modal, inner_width, (sel, sel))
}

fn format_horizontal_option(opt: &ModalOption, is_selected: bool, theme: &crate::ui::theme::Theme) -> String {
    if is_selected {
        let hl = theme.highlight;
        format!("{hl}▸{hl:#} \x1b[1m{}\x1b[0m", opt.label)
    } else {
        opt.label.clone()
    }
}

pub(super) fn format_horizontal_row(
    modal: &ModalState,
    layout: &ModalOptionsLayout<'_>,
    (start, end): (usize, usize),
) -> String {
    let mut parts = Vec::new();
    if start > 0 {
        parts.push(format!("{}‹{} ", layout.theme.dimmed, layout.theme.dimmed));
    }
    for i in start..=end {
        parts.push(format_horizontal_option(
            &modal.options[i],
            i == modal.selected,
            layout.theme,
        ));
        if i < end {
            parts.push("   ".to_string());
        }
    }
    if end + 1 < modal.options.len() {
        parts.push(format!(" {}›{}", layout.theme.dimmed, layout.theme.dimmed));
    }
    parts.concat()
}

pub(super) fn render_horizontal_options(modal: &ModalState, layout: &ModalOptionsLayout<'_>) -> Vec<String> {
    if layout.max_visible == 0 {
        return Vec::new();
    }
    if modal.options.is_empty() {
        let msg = if modal.is_searchable {
            "No matching models found"
        } else {
            "No matching options found"
        };
        let dimmed = layout.theme.dimmed;
        return vec![format!("    {dimmed}{msg}{dimmed:#}")];
    }
    let window = calculate_horizontal_window(modal, layout.inner_width);
    let row = format_horizontal_row(modal, layout, window);
    vec![format!("  {row}")]
}
