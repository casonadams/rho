//! Diff presentation formatting for tool invocations and interactive edits.

mod line;
mod token;
mod word;

#[cfg(test)]
mod tests;

pub use line::find_edit_line_number;
pub use word::{render_single_line_word_diff, replace_tabs};

use crate::ui::theme::Theme;

#[derive(Debug, Clone, Copy)]
pub struct EntryDiffInput<'a> {
    pub idx: usize,
    pub old_text: &'a str,
    pub new_text: &'a str,
    pub theme: &'a Theme,
    pub start_line: Option<usize>,
}

fn push_edit_header(out: &mut String, idx: usize, start_line: Option<usize>, dim: anstyle::Style) {
    if idx == 0 {
        return;
    }
    if let Some(line) = start_line {
        out.push_str(&format!("{dim}@@ edit #{} · line {line} @@{dim:#}\n", idx + 1));
    } else {
        out.push_str(&format!("{dim}@@ edit #{} @@{dim:#}\n", idx + 1));
    }
}

fn push_single_line_diff(
    out: &mut String,
    old_line: &str,
    new_line: &str,
    start_line: Option<usize>,
    gutter_width: usize,
    theme: &Theme,
) {
    let (removed, added) = render_single_line_word_diff(old_line, new_line, theme);
    if let Some(line) = start_line {
        let dim = theme.dimmed;
        out.push_str(&format!("{dim}{line:>gutter_width$} │ {dim:#}{removed}"));
        out.push_str(&format!("{dim}{line:>gutter_width$} │ {dim:#}{added}"));
    } else {
        out.push_str(&removed);
        out.push_str(&added);
    }
}

fn push_diff_lines(
    out: &mut String,
    lines: &[&str],
    is_add: bool,
    start_line: Option<usize>,
    gutter_width: usize,
    theme: &Theme,
) {
    let (prefix, color) = if is_add {
        ('+', theme.tool_ok)
    } else {
        ('-', theme.tool_err)
    };
    let dim = theme.dimmed;
    for (offset, line) in lines.iter().take(8).enumerate() {
        let clean = replace_tabs(line);
        if let Some(start) = start_line {
            let line_num = start + offset;
            out.push_str(&format!(
                "{dim}{line_num:>gutter_width$} │ {dim:#}{color}{prefix} {clean}{color:#}\n"
            ));
        } else {
            out.push_str(&format!("{color}{prefix} {clean}{color:#}\n"));
        }
    }
    if lines.len() > 8 {
        out.push_str(&format!("{dim}... ({} more lines){dim:#}\n", lines.len() - 8));
    }
}

fn push_multi_line_diff(
    out: &mut String,
    old_lines: &[&str],
    new_lines: &[&str],
    start_line: Option<usize>,
    gutter_width: usize,
    theme: &Theme,
) {
    push_diff_lines(out, old_lines, false, start_line, gutter_width, theme);
    push_diff_lines(out, new_lines, true, start_line, gutter_width, theme);
}

pub fn format_entry_diff(input: EntryDiffInput<'_>) -> String {
    let mut out = String::new();
    push_edit_header(&mut out, input.idx, input.start_line, input.theme.dimmed);

    let old_lines: Vec<&str> = input.old_text.lines().collect();
    let new_lines: Vec<&str> = input.new_text.lines().collect();
    let max_line = input
        .start_line
        .map(|start| start + old_lines.len().max(new_lines.len()))
        .unwrap_or(0);
    let gutter_width = max_line.to_string().len().max(3);

    if old_lines.len() == 1 && new_lines.len() == 1 {
        push_single_line_diff(
            &mut out,
            old_lines[0],
            new_lines[0],
            input.start_line,
            gutter_width,
            input.theme,
        );
    } else {
        push_multi_line_diff(
            &mut out,
            &old_lines,
            &new_lines,
            input.start_line,
            gutter_width,
            input.theme,
        );
    }

    out
}
