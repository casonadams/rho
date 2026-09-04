//! Diff presentation formatting for tool invocations and interactive edits.

mod token;
mod word;

pub use word::{render_single_line_word_diff, replace_tabs};

use crate::ui::theme::Theme;

#[derive(Debug, Clone, Copy)]
pub struct EntryDiffInput<'a> {
    pub idx: usize,
    pub old_text: &'a str,
    pub new_text: &'a str,
    pub theme: &'a Theme,
}

pub fn format_entry_diff(input: EntryDiffInput<'_>) -> String {
    let mut out = String::new();
    if input.idx > 0 {
        let dim = input.theme.dimmed;
        out.push_str(&format!("{dim}@@ edit #{} @@{dim:#}\n", input.idx + 1));
    }

    let old_lines: Vec<&str> = input.old_text.lines().collect();
    let new_lines: Vec<&str> = input.new_text.lines().collect();

    if old_lines.len() == 1 && new_lines.len() == 1 {
        let (removed, added) = render_single_line_word_diff(old_lines[0], new_lines[0], input.theme);
        out.push_str(&removed);
        out.push_str(&added);
    } else {
        let red = input.theme.tool_err;
        for line in old_lines.iter().take(8) {
            let clean = replace_tabs(line);
            out.push_str(&format!("{red}- {clean}{red:#}\n"));
        }
        if old_lines.len() > 8 {
            let dim = input.theme.dimmed;
            out.push_str(&format!("{dim}... ({} more lines){dim:#}\n", old_lines.len() - 8));
        }

        let green = input.theme.tool_ok;
        for line in new_lines.iter().take(8) {
            let clean = replace_tabs(line);
            out.push_str(&format!("{green}+ {clean}{green:#}\n"));
        }
        if new_lines.len() > 8 {
            let dim = input.theme.dimmed;
            out.push_str(&format!("{dim}... ({} more lines){dim:#}\n", new_lines.len() - 8));
        }
    }

    out
}
