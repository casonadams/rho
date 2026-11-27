use crate::ui::interactive::CursorPosition;
use crate::ui::interactive::a2ui::render::{clamp_line, more_lines_notice};
use crate::ui::interactive::footer::truncate_to_width;
use crate::ui::theme::Theme;

pub(super) fn window_around_focus(
    lines: Vec<String>,
    focus_range: Option<(usize, usize)>,
    max_lines: usize,
    width: usize,
    theme: &Theme,
) -> (Vec<String>, usize) {
    if lines.len() <= max_lines {
        return (lines, 0);
    }
    let keep = max_lines.saturating_sub(1).max(1);
    let focus_mid = focus_range.map_or(0, |(s, e)| (s + e) / 2);
    let start = focus_mid.saturating_sub(keep / 2).min(lines.len().saturating_sub(keep));
    let mut visible: Vec<String> = lines.iter().skip(start).take(keep).cloned().collect();
    let hidden = lines.len().saturating_sub(visible.len());
    if hidden > 0 && visible.len() < max_lines {
        let notice = more_lines_notice(hidden, theme);
        visible.push(clamp_line(&truncate_to_width(&notice, width), width));
    }
    (visible, start)
}

pub(super) fn adjust_cursor(
    cursor: Option<CursorPosition>,
    offset: usize,
    visible_len: usize,
    focused: bool,
) -> (CursorPosition, bool) {
    if let Some(c) = cursor
        && c.row >= offset
        && c.row - offset < visible_len
    {
        return (
            CursorPosition {
                row: c.row - offset,
                column: c.column,
            },
            focused,
        );
    }
    (CursorPosition { row: 0, column: 0 }, false)
}
