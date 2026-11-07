//! Markdown table parsing, column constraints, and layout.

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::ui::theme::Theme;
use crate::ui::wrap::{WordWrap, push_line_into};

const MIN_COLUMN_WIDTH: usize = 5;

struct TableFormat<'a> {
    pub widths: &'a [usize],
    pub theme: &'a Theme,
}

impl TableFormat<'_> {
    pub fn border(&self, left: char, mid: char, right: char) -> String {
        let mut border = String::from(left);
        for (index, width) in self.widths.iter().enumerate() {
            border.push_str(&"─".repeat(width + 2));
            border.push(if index + 1 < self.widths.len() { mid } else { right });
        }
        let dim = self.theme.dimmed;
        format!("{dim}{border}{dim:#}")
    }

    fn render_row_subline(&self, wrapped: &[Vec<String>], line_idx: usize, header: bool) -> String {
        let border = self.theme.dimmed;
        let bold = anstyle::Style::new().bold();
        let mut out = format!("{border}│{border:#} ");
        for (col, width) in self.widths.iter().enumerate() {
            let cell = wrapped[col].get(line_idx).map(String::as_str).unwrap_or("");
            let styled = if header {
                format!("{bold}{cell}{bold:#}")
            } else {
                cell.to_string()
            };
            let pad = " ".repeat(width.saturating_sub(UnicodeWidthStr::width(cell)));
            let sep = if col + 1 < self.widths.len() { " " } else { "" };
            out.push_str(&format!("{styled}{pad} {border}│{border:#}{sep}"));
        }
        out.push('\n');
        out
    }

    pub fn row(&self, row: &[String], header: bool) -> String {
        let wrapped: Vec<Vec<String>> = self
            .widths
            .iter()
            .enumerate()
            .map(|(i, w)| wrap_cell(row.get(i).map(String::as_str).unwrap_or(""), *w))
            .collect();
        let height = wrapped.iter().map(Vec::len).max().unwrap_or(1);
        let mut output = String::new();
        for line_idx in 0..height {
            output.push_str(&self.render_row_subline(&wrapped, line_idx, header));
        }
        output
    }
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

struct CellWrapper {
    lines: Vec<String>,
    current: String,
    word: WordWrap,
}

impl CellWrapper {
    fn new(width: usize) -> Self {
        Self {
            lines: Vec::new(),
            current: String::new(),
            word: WordWrap::new(width.max(1)),
        }
    }

    fn commit_word(&mut self) {
        let keep_leading = self.lines.is_empty();
        self.word
            .commit(&mut self.current, keep_leading, push_line_into(&mut self.lines));
    }

    fn push_space(&mut self, character: char, character_width: usize) {
        self.commit_word();
        self.word.push_space(character, character_width);
    }

    fn push_char(&mut self, character: char, character_width: usize) {
        if self.word.pending_word_width + character_width > self.word.width {
            if self.word.current_width > 0 {
                let lines = &mut self.lines;
                self.word
                    .break_line(&mut self.current, |current| lines.push(std::mem::take(current)));
                self.word.clear_spaces();
            }
            if self.word.pending_word_width + character_width > self.word.width && self.word.pending_word_width > 0 {
                self.lines.push(std::mem::take(&mut self.word.pending_word));
                self.word.pending_word_width = 0;
            }
        }
        self.word.push_word_char(character, character_width);
    }

    fn finish(mut self) -> Vec<String> {
        self.commit_word();
        if !self.current.is_empty() || self.lines.is_empty() {
            self.lines.push(self.current);
        }
        self.lines
    }
}

fn wrap_cell(cell: &str, width: usize) -> Vec<String> {
    let mut wrapper = CellWrapper::new(width);
    for character in cell.chars() {
        let character_width = UnicodeWidthChar::width(character).unwrap_or(0);
        if character == ' ' || character == '\t' {
            wrapper.push_space(character, character_width);
        } else {
            wrapper.push_char(character, character_width);
        }
    }
    wrapper.finish()
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
    let mut out = format!("{}\n", table.border('╭', '┬', '╮'));
    for (idx, row) in rows.iter().enumerate() {
        out.push_str(&table.row(row, idx < divider_index));
        if idx + 1 < rows.len() {
            out.push_str(&format!("{}\n", table.border('├', '┼', '┤')));
        }
    }
    out.push_str(&format!("{}\n", table.border('╰', '┴', '╯')));
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

#[cfg(test)]
mod tests {
    use super::wrap_cell;

    #[test]
    fn wrap_cell_breaks_words_at_the_column_width() {
        let cases = [
            ("", 5, vec![""]),
            ("ab", 0, vec!["a", "b"]),
            ("ab cd", 10, vec!["ab cd"]),
            ("abc defgh", 5, vec!["abc", "defgh"]),
            ("abc defghij", 5, vec!["abc", "defgh", "ij"]),
            ("abcdefghijkl", 5, vec!["abcde", "fghij", "kl"]),
        ];
        for (cell, width, expected) in cases {
            assert_eq!(wrap_cell(cell, width), expected, "cell {cell:?} at width {width}");
        }
    }
}
