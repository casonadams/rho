use unicode_width::UnicodeWidthChar;

use crate::ui::interactive::state::paste::find_marker_covering;

type CharIndicesItr<'a> = std::iter::Peekable<std::str::CharIndices<'a>>;

pub(crate) fn skip_whitespace_back(slice: &str) -> std::iter::Peekable<std::iter::Rev<std::str::CharIndices<'_>>> {
    let mut chars = slice.char_indices().rev().peekable();
    while let Some((_, c)) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else {
            break;
        }
    }
    chars
}

pub(crate) fn skip_whitespace_fwd(slice: &str) -> CharIndicesItr<'_> {
    let mut chars = slice.char_indices().peekable();
    while let Some((_, c)) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else {
            break;
        }
    }
    chars
}

pub(crate) fn char_class(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

pub(crate) fn snap_to_marker_end(text: &str, cursor: &mut usize, start: bool) {
    if let Some(marker) = find_marker_covering(text, *cursor) {
        *cursor = if start { marker.start } else { marker.end };
    }
}

pub(crate) fn editor_boundaries(text: &str) -> impl Iterator<Item = usize> + '_ {
    std::iter::once(0).chain(
        text.char_indices()
            .map(|(index, character)| index + character.len_utf8()),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct VisualLine {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) width: usize,
}

fn display_width(slice: &str) -> usize {
    slice.chars().map(|c| c.width().unwrap_or(0)).sum()
}

fn push_chars_hard_wrap(
    text: &str,
    range: std::ops::Range<usize>,
    max_width: usize,
    line_start: &mut usize,
    line_width: &mut usize,
    out: &mut Vec<VisualLine>,
) {
    for (rel_idx, c) in text[range.clone()].char_indices() {
        let abs_idx = range.start + rel_idx;
        let cw = c.width().unwrap_or(0);
        if *line_width > 0 && *line_width + cw > max_width {
            out.push(VisualLine {
                start: *line_start,
                end: abs_idx,
                width: *line_width,
            });
            *line_start = abs_idx;
            *line_width = 0;
        }
        *line_width += cw;
    }
}

fn wrap_logical_line(
    text: &str,
    logical_start: usize,
    logical_end: usize,
    max_width: usize,
    out: &mut Vec<VisualLine>,
) {
    let mut line_start = logical_start;
    let mut line_width = 0usize;
    let mut pos = logical_start;

    while pos < logical_end {
        let rest = &text[pos..logical_end];
        let is_ws = rest.chars().next().is_some_and(char::is_whitespace);
        let run_len = rest
            .char_indices()
            .find(|(_, c)| c.is_whitespace() != is_ws)
            .map_or(rest.len(), |(idx, _)| idx);
        let run_end = pos + run_len;

        if !is_ws {
            let word_width = display_width(&text[pos..run_end]);
            if line_width > 0 && line_width + word_width > max_width {
                out.push(VisualLine {
                    start: line_start,
                    end: pos,
                    width: line_width,
                });
                line_start = pos;
                line_width = 0;
            }
        }
        push_chars_hard_wrap(text, pos..run_end, max_width, &mut line_start, &mut line_width, out);
        pos = run_end;
    }

    out.push(VisualLine {
        start: line_start,
        end: logical_end,
        width: line_width,
    });
}

pub(crate) fn wrap_visual_lines(text: &str, max_width: usize) -> Vec<VisualLine> {
    let max_width = max_width.max(1);
    let mut out = Vec::new();
    let mut logical_start = 0usize;
    for (idx, c) in text.char_indices() {
        if c == '\n' {
            wrap_logical_line(text, logical_start, idx, max_width, &mut out);
            logical_start = idx + 1;
        }
    }
    wrap_logical_line(text, logical_start, text.len(), max_width, &mut out);
    out
}

pub(crate) fn cursor_position_in_lines(
    text: &str,
    lines: &[VisualLine],
    cursor: usize,
    max_width: usize,
) -> (usize, usize) {
    let max_width = max_width.max(1);
    if cursor == text.len() {
        let last_row = lines.len().saturating_sub(1);
        let last_width = lines.last().map_or(0, |line| line.width);
        return if last_width >= max_width {
            (lines.len(), 0)
        } else {
            (last_row, last_width)
        };
    }
    for (row, line) in lines.iter().enumerate() {
        let on_line = cursor >= line.start && cursor < line.end;
        let on_newline = cursor == line.end && text.as_bytes().get(line.end) == Some(&b'\n');
        if on_line || on_newline {
            return (row, display_width(&text[line.start..cursor]));
        }
    }
    (lines.len().saturating_sub(1), lines.last().map_or(0, |l| l.width))
}

pub(crate) fn compute_vertical_target(
    text: &str,
    cursor: usize,
    terminal_width: usize,
    row_delta: isize,
    preferred_column: Option<usize>,
) -> Option<(usize, usize)> {
    let terminal_width = terminal_width.max(1);
    let lines = wrap_visual_lines(text, terminal_width);
    let (current_row, current_column) = cursor_position_in_lines(text, &lines, cursor, terminal_width);
    let target_row = current_row.checked_add_signed(row_delta)?;
    let pref_col = preferred_column.unwrap_or(current_column);
    let target = editor_boundaries(text)
        .map(|cur| {
            let (row, column) = cursor_position_in_lines(text, &lines, cur, terminal_width);
            (cur, row, column)
        })
        .filter(|(_, row, _)| *row == target_row)
        .min_by_key(|(_, _, column)| column.abs_diff(pref_col));

    let (mut new_cursor, _, _) = target?;
    if let Some(marker) = find_marker_covering(text, new_cursor) {
        let to_start = new_cursor - marker.start;
        let to_end = marker.end - new_cursor;
        new_cursor = if to_start <= to_end { marker.start } else { marker.end };
    }
    Some((new_cursor, pref_col))
}
