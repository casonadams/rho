use crate::ui::block::{truncate_to_width, visible_width};

pub fn sanitize_status_text(text: &str) -> String {
    let single_line = text
        .chars()
        .map(|c| if c == '\r' || c == '\n' || c == '\t' { ' ' } else { c })
        .collect::<String>();
    let mut words = single_line.split_whitespace();
    let mut result = String::new();
    if let Some(first) = words.next() {
        result.push_str(first);
        for word in words {
            result.push(' ');
            result.push_str(word);
        }
    }
    result
}

pub fn truncate_with_ellipsis(value: &str, width: usize) -> String {
    if visible_width(value) <= width {
        return value.to_string();
    }
    if width <= 3 {
        return truncate_to_width(value, width);
    }
    let target = width - 3;
    let truncated = truncate_to_width(value, target);
    format!("{truncated}...")
}

pub fn fit_right_aligned(left: &str, right: &str, width: usize) -> String {
    let right_width = visible_width(right);
    let safe_right = if right_width > width {
        truncate_to_width(right, width)
    } else {
        right.to_string()
    };
    let safe_right_width = visible_width(&safe_right);

    let left_width = visible_width(left);
    if left_width + safe_right_width + 2 <= width {
        let padding = width.saturating_sub(left_width + safe_right_width);
        return format!("{left}{}{safe_right}", " ".repeat(padding));
    }

    let available_left = width.saturating_sub(safe_right_width + 2);
    let truncated_left = if available_left > 0 {
        truncate_with_ellipsis(left, available_left)
    } else {
        String::new()
    };
    let truncated_left_width = visible_width(&truncated_left);
    let padding = width.saturating_sub(truncated_left_width + safe_right_width);
    format!("{truncated_left}{}{safe_right}", " ".repeat(padding))
}
