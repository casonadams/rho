use crate::ui::interactive::state::autocomplete::AutocompleteState;
use unicode_width::UnicodeWidthStr;

pub(crate) const MAX_VISIBLE_ITEMS: usize = 7;

pub(crate) fn render_autocomplete_dropdown(state: &AutocompleteState, width: usize, max_lines: usize) -> Vec<String> {
    if !state.visible || state.items.is_empty() || width < 15 || max_lines < 2 {
        return Vec::new();
    }

    let total = state.items.len();
    let visible_count = total.min(MAX_VISIBLE_ITEMS).min(max_lines);

    let start = if total <= visible_count || state.selected < visible_count / 2 {
        0
    } else if state.selected + (visible_count - visible_count / 2) >= total {
        total - visible_count
    } else {
        state.selected - visible_count / 2
    };

    let mut lines = Vec::new();
    let inner_width = width.saturating_sub(4);

    for idx in start..start + visible_count {
        let item = &state.items[idx];
        let is_selected = idx == state.selected;

        let prefix = if is_selected { "\x1b[1;36m>\x1b[0m " } else { "  " };

        let val_styled = if is_selected {
            format!("\x1b[1;37m{}\x1b[0m", item.value)
        } else {
            format!("\x1b[36m{}\x1b[0m", item.value)
        };

        let desc_str = item.description.as_deref().unwrap_or("");
        let val_width = UnicodeWidthStr::width(item.value.as_str()) + 2; // +2 for prefix

        let line = if val_width + 3 < inner_width && !desc_str.is_empty() {
            let available_desc_width = inner_width.saturating_sub(val_width + 2);
            let truncated_desc = truncate_width(desc_str, available_desc_width);
            let desc_styled = format!("\x1b[2m{}\x1b[0m", truncated_desc);
            let padding =
                " ".repeat(inner_width.saturating_sub(val_width + 2 + UnicodeWidthStr::width(truncated_desc.as_str())));
            format!(" {prefix}{val_styled}  {desc_styled}{padding}")
        } else {
            let padding = " ".repeat(inner_width.saturating_sub(val_width));
            format!(" {prefix}{val_styled}{padding}")
        };

        if is_selected {
            lines.push(format!("\x1b[48;5;236m{line}\x1b[0m"));
        } else {
            lines.push(line);
        }
    }

    lines
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
