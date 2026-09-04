//! Markdown table layout, cell wrapping, and borders.

use crate::ui::theme::Theme;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub(super) const MIN_COLUMN_WIDTH: usize = 5;

pub(super) struct TableFormat<'a> {
    pub widths: &'a [usize],
    pub theme: &'a Theme,
}

impl TableFormat<'_> {
    pub fn border(&self, (left, mid, right): (char, char, char)) -> String {
        let mut border = String::from(left);
        for (index, width) in self.widths.iter().enumerate() {
            border.push_str(&"─".repeat(width + 2));
            border.push(if index + 1 < self.widths.len() { mid } else { right });
        }
        let dim = self.theme.dimmed;
        format!("{dim}{border}{dim:#}")
    }

    pub fn row(&self, row: &[String], header: bool) -> String {
        let wrapped: Vec<Vec<String>> = self
            .widths
            .iter()
            .enumerate()
            .map(|(i, w)| wrap_cell(row.get(i).map(String::as_str).unwrap_or(""), *w))
            .collect();
        let height = wrapped.iter().map(Vec::len).max().unwrap_or(1);
        let border = self.theme.dimmed;
        let bold = anstyle::Style::new().bold();
        let mut output = String::new();
        for line_idx in 0..height {
            output.push_str(&format!("{border}│{border:#} "));
            for (col, width) in self.widths.iter().enumerate() {
                let cell = wrapped[col].get(line_idx).map(String::as_str).unwrap_or("");
                let styled = if header {
                    format!("{bold}{cell}{bold:#}")
                } else {
                    cell.to_string()
                };
                let pad = " ".repeat(width.saturating_sub(UnicodeWidthStr::width(cell)));
                let sep = if col + 1 < self.widths.len() { " " } else { "" };
                output.push_str(&format!("{styled}{pad} {border}│{border:#}{sep}"));
            }
            output.push('\n');
        }
        output
    }
}

pub(super) fn constrain_column_widths(widths: &mut [usize], available: usize) {
    while widths.iter().sum::<usize>() > available {
        let Some((index, _)) = widths.iter().enumerate().max_by_key(|(_, width)| *width) else {
            return;
        };
        if widths[index] <= MIN_COLUMN_WIDTH {
            return;
        }
        widths[index] -= 1;
    }
}

pub(super) fn wrap_cell(cell: &str, width: usize) -> Vec<String> {
    let mut lines = vec![String::new()];
    let mut current_width = 0;
    for character in cell.chars() {
        let char_width = UnicodeWidthChar::width(character).unwrap_or(0);
        if current_width > 0 && current_width + char_width > width {
            lines.push(String::new());
            current_width = 0;
        }
        lines.last_mut().unwrap().push(character);
        current_width += char_width;
    }
    lines
}

pub(super) fn render_compact_table(rows: &[Vec<String>], header_end: usize, width: usize) -> String {
    let bold = anstyle::Style::new().bold();
    let mut output = String::new();
    for (index, row) in rows.iter().enumerate() {
        let joined = row.join(" | ");
        for line in wrap_cell(&joined, width.max(1)) {
            if index < header_end {
                output.push_str(&format!("{bold}{line}{bold:#}\n"));
            } else {
                output.push_str(&line);
                output.push('\n');
            }
        }
    }
    output
}

pub(super) fn render_table_fallback(lines: &[String], theme: &Theme) -> String {
    let mut output = String::new();
    for line in lines {
        output.push_str(&super::super::elements::render_inline_elements(line, theme));
        output.push('\n');
    }
    output
}
