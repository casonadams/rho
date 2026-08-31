//! Inline-element rendering (pulldown-cmark), mermaid blocks, and table formatting.

use crate::ui::theme::Theme;
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const MIN_COLUMN_WIDTH: usize = 5;

pub fn render_inline_elements(text: &str, theme: &Theme) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);

    let parser = Parser::new_ext(text, options);
    let mut out = String::new();

    let bold_style = anstyle::Style::new().bold();
    let italic_style = anstyle::Style::new().italic();
    let strike_style = anstyle::Style::new().strikethrough();

    for event in parser {
        match event {
            Event::Start(tag) => match tag {
                Tag::Strong => out.push_str(&bold_style.render().to_string()),
                Tag::Emphasis => out.push_str(&italic_style.render().to_string()),
                Tag::Strikethrough => out.push_str(&strike_style.render().to_string()),
                Tag::Link { .. } => out.push_str(&theme.highlight.render().to_string()),
                _ => {}
            },
            Event::End(tag_end) => match tag_end {
                TagEnd::Strong => out.push_str(&bold_style.render_reset().to_string()),
                TagEnd::Emphasis => out.push_str(&italic_style.render_reset().to_string()),
                TagEnd::Strikethrough => out.push_str(&strike_style.render_reset().to_string()),
                TagEnd::Link => out.push_str(&theme.highlight.render_reset().to_string()),
                _ => {}
            },
            Event::Text(t) => out.push_str(&t),
            Event::Code(c) => {
                let code = theme.code_inline;
                out.push_str(&format!("{code}{c}{code:#}"));
            }
            Event::SoftBreak => out.push(' '),
            Event::HardBreak => out.push('\n'),
            _ => {}
        }
    }

    let trailing_spaces = text.len() - text.trim_end_matches(' ').len();
    if trailing_spaces > 0 && !out.ends_with(' ') {
        for _ in 0..trailing_spaces {
            out.push(' ');
        }
    }

    out
}

pub fn render_mermaid_block(source: &str, theme: &Theme) -> String {
    let header = theme.tool_header;
    let dim = theme.dimmed;

    let mut out = format!("\n{header}[mermaid diagram]{header:#}\n");
    match meraid::render(source, meraid::ThemeType::default()) {
        Ok(rendered) => {
            for line in rendered.lines() {
                out.push_str(&format!("{line}\n"));
            }
        }
        Err(_) => {
            for line in source.lines() {
                out.push_str(&format!("{dim}│{dim:#} {line}\n"));
            }
        }
    }
    out.push('\n');
    out
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
        && stripped.chars().all(|c| c == '|' || c == '-' || c == ':')
}

pub fn render_markdown_table(lines: &[String], theme: &Theme) -> String {
    let width = crossterm::terminal::size()
        .map(|(columns, _)| usize::from(columns.saturating_sub(2)))
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
    output.push('\n');
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
    output.push_str("\n\n");
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
    fn border(&self, edges: (char, char, char)) -> String {
        let (left, middle, right) = edges;
        let mut border = String::new();
        border.push(left);
        for (index, width) in self.widths.iter().enumerate() {
            border.push_str(&"─".repeat(width + 2));
            border.push(if index + 1 < self.widths.len() { middle } else { right });
        }
        let dim = self.theme.dimmed;
        format!("{dim}{border}{dim:#}")
    }

    fn row(&self, row: &[String], header: bool) -> String {
        let wrapped: Vec<Vec<String>> = self
            .widths
            .iter()
            .enumerate()
            .map(|(index, width)| wrap_cell(row.get(index).map(String::as_str).unwrap_or(""), *width))
            .collect();
        let height = wrapped.iter().map(Vec::len).max().unwrap_or(1);
        let border = self.theme.dimmed;
        let bold = anstyle::Style::new().bold();
        let mut output = String::new();
        for line_index in 0..height {
            output.push_str(&format!("{border}│{border:#} "));
            for (column, width) in self.widths.iter().enumerate() {
                let content = wrapped[column].get(line_index).map(String::as_str).unwrap_or("");
                if header {
                    output.push_str(&format!("{bold}{content}{bold:#}"));
                } else {
                    output.push_str(content);
                }
                output.push_str(&" ".repeat(width.saturating_sub(UnicodeWidthStr::width(content))));
                output.push_str(&format!(" {border}│{border:#}"));
                if column + 1 < self.widths.len() {
                    output.push(' ');
                }
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
        let character_width = UnicodeWidthChar::width(character).unwrap_or(0);
        if current_width > 0 && current_width + character_width > width {
            lines.push(String::new());
            current_width = 0;
        }
        lines.last_mut().unwrap().push(character);
        current_width += character_width;
    }
    lines
}

fn render_compact_table(rows: &[Vec<String>], header_end: usize, width: usize) -> String {
    let bold = anstyle::Style::new().bold();
    let mut output = String::from("\n");
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
    output.push('\n');
    output
}

fn render_table_fallback(lines: &[String], theme: &Theme) -> String {
    let mut output = String::new();
    for line in lines {
        output.push_str(&render_inline_elements(line, theme));
        output.push('\n');
    }
    output
}

pub fn strip_markdown_decorations(s: &str) -> String {
    s.replace("**", "").replace(['*', '`'], "").trim().to_string()
}
