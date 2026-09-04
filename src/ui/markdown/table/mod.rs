//! Markdown table parsing, column constraints, and layout.

mod format;

use crate::ui::theme::Theme;
use format::{MIN_COLUMN_WIDTH, TableFormat, constrain_column_widths, render_compact_table, render_table_fallback};
use unicode_width::UnicodeWidthStr;

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

pub fn strip_markdown_decorations(s: &str) -> String {
    s.replace("**", "").replace(['*', '`'], "").trim().to_string()
}
