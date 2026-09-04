//! Markdown table rendering and layout utilities.

use crate::ui::theme::Theme;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const MIN_COLUMN_WIDTH: usize = 5;

pub fn is_table_line(trimmed: &str) -> bool {
    (trimmed.starts_with('|') && trimmed.ends_with('|') && trimmed.len() >= 2) || is_table_divider(trimmed)
}

pub fn is_table_divider(line: &str) -> bool {
    let stripped: String = line.chars().filter(|c| !c.is_whitespace()).collect();
    stripped.starts_with('|')
        && stripped.ends_with('|')
        && stripped.len() >= 3
        && stripped.contains('-')
        && stripped.chars().all(|c| matches!(c, '|' | '-' | ':'))
}

pub fn render_markdown_table(lines: &[String], theme: &Theme) -> String {
    let width = crossterm::terminal::size()
        .map(|(cols, _)| usize::from(cols.saturating_sub(2)).max(40))
        .unwrap_or(78);
    render_markdown_table_at_width(lines, theme, width)
}

pub(crate) fn render_markdown_table_at_width(lines: &[String], theme: &Theme, width: usize) -> String {
    let Some(divider_index) = lines.iter().position(|line| is_table_divider(line.trim())) else {
        return render_table_fallback(lines, theme);
    };
    let rows: Vec<Vec<String>> = lines
        .iter()
        .filter(|line| !is_table_divider(line.trim()))
        .map(|line| {
            line.trim()
                .trim_matches('|')
                .split('|')
                .map(|cell| strip_markdown_decorations(cell.trim()))
                .collect()
        })
        .collect();
    let column_count = rows.iter().map(Vec::len).max().unwrap_or(0);
    if column_count == 0 {
        return String::new();
    }
    let overhead = column_count * 3 + 1;
    if width < overhead + column_count * MIN_COLUMN_WIDTH {
        return render_compact_table(&rows, divider_index, width);
    }

    let mut column_widths = vec![MIN_COLUMN_WIDTH; column_count];
    for row in &rows {
        for (column, cell) in row.iter().enumerate() {
            column_widths[column] = column_widths[column].max(UnicodeWidthStr::width(cell.as_str()));
        }
    }
    constrain_column_widths(&mut column_widths, width - overhead);

    let table = TableFormat {
        widths: &column_widths,
        theme,
    };
    let mut output = String::new();
    output.push_str(&table.border(('╭', '┬', '╮')));
    output.push('\n');
    for (row_index, row) in rows.iter().enumerate() {
        output.push_str(&table.row(row, row_index < divider_index));
        if row_index + 1 < rows.len() {
            output.push_str(&table.border(('├', '┼', '┤')));
            output.push('\n');
        }
    }
    output.push_str(&table.border(('╰', '┴', '╯')));
    output.push('\n');
    output
}

fn constrain_column_widths(widths: &mut [usize], available: usize) {
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

struct TableFormat<'a> {
    widths: &'a [usize],
    theme: &'a Theme,
}

impl TableFormat<'_> {
    fn border(&self, (left, mid, right): (char, char, char)) -> String {
        let mut border = String::from(left);
        for (index, width) in self.widths.iter().enumerate() {
            border.push_str(&"─".repeat(width + 2));
            border.push(if index + 1 < self.widths.len() { mid } else { right });
        }
        let dim = self.theme.dimmed;
        format!("{dim}{border}{dim:#}")
    }

    fn row(&self, row: &[String], header: bool) -> String {
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

fn wrap_cell(cell: &str, width: usize) -> Vec<String> {
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

fn render_compact_table(rows: &[Vec<String>], header_end: usize, width: usize) -> String {
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

fn render_table_fallback(lines: &[String], theme: &Theme) -> String {
    let mut output = String::new();
    for line in lines {
        output.push_str(&super::elements::render_inline_elements(line, theme));
        output.push('\n');
    }
    output
}

pub fn strip_markdown_decorations(s: &str) -> String {
    s.replace("**", "").replace(['*', '`'], "").trim().to_string()
}
