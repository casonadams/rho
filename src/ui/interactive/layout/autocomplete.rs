use crate::ui::interactive::state::autocomplete::{AutocompleteItem, AutocompleteState};
use unicode_width::UnicodeWidthStr;

pub(crate) const MAX_VISIBLE_ITEMS: usize = 7;

fn calculate_scroll_range(total: usize, selected: usize, max_visible: usize) -> (usize, usize) {
    let visible_count = total.min(MAX_VISIBLE_ITEMS).min(max_visible);
    let start = if total <= visible_count || selected < visible_count / 2 {
        0
    } else if selected + (visible_count - visible_count / 2) >= total {
        total - visible_count
    } else {
        selected - visible_count / 2
    };
    (start, visible_count)
}

fn build_item_content(
    (item, theme): (&AutocompleteItem, &crate::ui::theme::Theme),
    (val_styled, prefix): (&str, &str),
    (val_width, inner_width): (usize, usize),
) -> String {
    let desc = item.description.as_deref().unwrap_or("");
    if val_width + 3 < inner_width && !desc.is_empty() {
        let available = inner_width.saturating_sub(val_width + 2);
        let truncated = truncate_width(desc, available);
        let desc_styled = format!("{}{}{:#}", theme.dimmed, truncated, theme.dimmed);
        let pad_len = inner_width.saturating_sub(val_width + 2 + UnicodeWidthStr::width(truncated.as_str()));
        format!(" {prefix}{val_styled}  {desc_styled}{}", " ".repeat(pad_len))
    } else {
        let padding = " ".repeat(inner_width.saturating_sub(val_width));
        format!(" {prefix}{val_styled}{padding}")
    }
}

fn format_dropdown_item(
    item: &AutocompleteItem,
    (is_selected, inner_width): (bool, usize),
    theme: &crate::ui::theme::Theme,
) -> String {
    let highlight = theme.highlight;
    let prefix = if is_selected {
        format!("{highlight}▸{highlight:#} ")
    } else {
        "  ".to_string()
    };
    let val_styled = if is_selected {
        format!("{}{}{:#}", highlight.bold(), item.value, highlight.bold())
    } else {
        format!("{}{}{:#}", theme.prompt, item.value, theme.prompt)
    };
    let val_width = UnicodeWidthStr::width(item.value.as_str()) + 2;
    build_item_content((item, theme), (&val_styled, &prefix), (val_width, inner_width))
}

pub(crate) fn render_autocomplete_dropdown(
    state: &AutocompleteState,
    bounds: (usize, usize),
    theme: &crate::ui::theme::Theme,
) -> Vec<String> {
    let (width, max_lines) = bounds;
    if !state.visible || state.items.is_empty() || width < 15 || max_lines < 2 {
        return Vec::new();
    }

    let (start, visible_count) = calculate_scroll_range(state.items.len(), state.selected, max_lines);
    let inner_width = width.saturating_sub(4);
    (start..start + visible_count)
        .map(|idx| format_dropdown_item(&state.items[idx], (idx == state.selected, inner_width), theme))
        .collect()
}

fn truncate_width(s: &str, max_width: usize) -> String {
    let mut current_width = 0;
    let mut result = String::new();
    for c in s.chars() {
        let w = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
        if current_width + w > max_width {
            break;
        }
        result.push(c);
        current_width += w;
    }
    result
}
