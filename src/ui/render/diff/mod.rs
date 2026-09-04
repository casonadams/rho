//! Diff presentation formatting for tool invocations and interactive edits.

mod line;
mod token;
mod word;

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

pub fn format_entry_diff(input: EntryDiffInput<'_>) -> String {
    let mut out = String::new();
    if input.idx > 0 {
        let dim = input.theme.dimmed;
        if let Some(line) = input.start_line {
            out.push_str(&format!("{dim}@@ edit #{} · line {line} @@{dim:#}\n", input.idx + 1));
        } else {
            out.push_str(&format!("{dim}@@ edit #{} @@{dim:#}\n", input.idx + 1));
        }
    }

    let old_lines: Vec<&str> = input.old_text.lines().collect();
    let new_lines: Vec<&str> = input.new_text.lines().collect();
    let max_line = input
        .start_line
        .map(|start| start + old_lines.len().max(new_lines.len()))
        .unwrap_or(0);
    let gutter_width = max_line.to_string().len().max(3);

    if old_lines.len() == 1 && new_lines.len() == 1 {
        let (removed, added) = render_single_line_word_diff(old_lines[0], new_lines[0], input.theme);
        if let Some(line) = input.start_line {
            let dim = input.theme.dimmed;
            out.push_str(&format!("{dim}{line:>gutter_width$} │ {dim:#}{removed}"));
            out.push_str(&format!("{dim}{line:>gutter_width$} │ {dim:#}{added}"));
        } else {
            out.push_str(&removed);
            out.push_str(&added);
        }
    } else {
        let red = input.theme.tool_err;
        let dim = input.theme.dimmed;
        for (offset, line) in old_lines.iter().take(8).enumerate() {
            let clean = replace_tabs(line);
            if let Some(start) = input.start_line {
                let line_num = start + offset;
                out.push_str(&format!(
                    "{dim}{line_num:>gutter_width$} │ {dim:#}{red}- {clean}{red:#}\n"
                ));
            } else {
                out.push_str(&format!("{red}- {clean}{red:#}\n"));
            }
        }
        if old_lines.len() > 8 {
            out.push_str(&format!("{dim}... ({} more lines){dim:#}\n", old_lines.len() - 8));
        }

        let green = input.theme.tool_ok;
        for (offset, line) in new_lines.iter().take(8).enumerate() {
            let clean = replace_tabs(line);
            if let Some(start) = input.start_line {
                let line_num = start + offset;
                out.push_str(&format!(
                    "{dim}{line_num:>gutter_width$} │ {dim:#}{green}+ {clean}{green:#}\n"
                ));
            } else {
                out.push_str(&format!("{green}+ {clean}{green:#}\n"));
            }
        }
        if new_lines.len() > 8 {
            out.push_str(&format!("{dim}... ({} more lines){dim:#}\n", new_lines.len() - 8));
        }
    }

    out
}
