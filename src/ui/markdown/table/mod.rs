//! Markdown table parsing, column constraints, and layout.

mod format;

use format::{MIN_COLUMN_WIDTH, TableFormat, constrain_column_widths, render_compact_table, render_table_fallback};
use unicode_width::UnicodeWidthStr;

use crate::ui::theme::Theme;

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

fn parse_table_rows(lines: &[String]) -> Vec<Vec<String>> {
    lines
        .iter()
        .filter(|line| !is_table_divider(line.trim()))
        .map(|line| {
            line.trim()
                .trim_matches('|')
                .split('|')
                .map(|cell| strip_markdown_decorations(cell.trim()))
                .collect()
        })
        .collect()
}

fn compute_column_widths(rows: &[Vec<String>], col_count: usize, max_budget: usize) -> Vec<usize> {
    let mut widths = vec![MIN_COLUMN_WIDTH; col_count];
    for row in rows {
        for (col, cell) in row.iter().enumerate() {
            widths[col] = widths[col].max(UnicodeWidthStr::width(cell.as_str()));
        }
    }
    constrain_column_widths(&mut widths, max_budget);
    widths
}

fn format_table_output(table: &TableFormat<'_>, rows: &[Vec<String>], divider_index: usize) -> String {
    let mut out = format!("{}\n", table.border(('╭', '┬', '╮')));
    for (idx, row) in rows.iter().enumerate() {
        out.push_str(&table.row(row, idx < divider_index));
        if idx + 1 < rows.len() {
            out.push_str(&format!("{}\n", table.border(('├', '┼', '┤'))));
        }
    }
    out.push_str(&format!("{}\n", table.border(('╰', '┴', '╯'))));
    out
}

pub(crate) fn render_markdown_table_at_width(lines: &[String], theme: &Theme, width: usize) -> String {
    let Some(divider_index) = lines.iter().position(|line| is_table_divider(line.trim())) else {
        return render_table_fallback(lines, theme);
    };
    let rows = parse_table_rows(lines);
    let col_count = rows.iter().map(Vec::len).max().unwrap_or(0);
    if col_count == 0 {
        return String::new();
    }
    let overhead = col_count * 3 + 1;
    if width < overhead + col_count * MIN_COLUMN_WIDTH {
        return render_compact_table(&rows, divider_index, width);
    }
    let widths = compute_column_widths(&rows, col_count, width - overhead);
    format_table_output(&TableFormat { widths: &widths, theme }, &rows, divider_index)
}

pub fn strip_markdown_decorations(s: &str) -> String {
    s.replace("**", "").replace(['*', '`'], "").trim().to_string()
}
