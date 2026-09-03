use crate::ui::interactive::state::autocomplete::AutocompleteState;
use unicode_width::UnicodeWidthStr;

const MAX_VISIBLE_ITEMS: usize = 7;

/// Renders a Pi-style compact fuzzy autocomplete menu.
/// Shows up to MAX_VISIBLE_ITEMS with selection indicator, command name, and description.
pub(crate) fn render_autocomplete_dropdown(state: &AutocompleteState, width: usize) -> Vec<String> {
    if !state.visible || state.items.is_empty() || width < 15 {
        return Vec::new();
    }

    let total = state.items.len();
    let visible_count = total.min(MAX_VISIBLE_ITEMS);

    // Calculate window slice to keep selected item in view
    let start = if total <= MAX_VISIBLE_ITEMS || state.selected < visible_count / 2 {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repl::interactive::Completion;
    use std::ops::Range;

    #[test]
    fn test_render_autocomplete_dropdown() {
        let mut state = AutocompleteState::default();
        let items = vec![
            Completion {
                value: "/model".to_string(),
                description: Some("Switch model".to_string()),
                replacement: Range { start: 0, end: 1 },
            },
            Completion {
                value: "/skill".to_string(),
                description: Some("Inspect skills".to_string()),
                replacement: Range { start: 0, end: 1 },
            },
        ];
        state.open(items);

        let lines = render_autocomplete_dropdown(&state, 60);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("/model"));
        assert!(lines[0].contains("Switch model"));
        assert!(lines[1].contains("/skill"));
    }

    #[test]
    fn descriptions_share_the_footer_dim_style() {
        // Descriptions must match the footer's plain faint (`Theme::dimmed`,
        // SGR 2) — stacking faint on gray (`2;90`) is unreadably dark.
        let footer_dim = crate::ui::theme::Theme::default().dimmed.render().to_string();
        let mut state = AutocompleteState::default();
        state.open(vec![Completion {
            value: "/model".to_string(),
            description: Some("Switch model".to_string()),
            replacement: Range { start: 0, end: 1 },
        }]);

        let lines = render_autocomplete_dropdown(&state, 60);
        assert!(lines[0].contains(&footer_dim), "{}", lines[0]);
        assert!(!lines[0].contains("\x1b[2;90m"), "{}", lines[0]);
    }
}
