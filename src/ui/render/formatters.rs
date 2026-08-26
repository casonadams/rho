//! Edit-diff, write-preview, and thinking-block formatters.
//!
//! These are `pub(crate)` because they are only consumed by `renderer.rs`,
//! but they remain exposed as module-private items so future tools can reuse them.

use crate::ui::theme::Theme;

struct DiffFormatter<'a> {
    theme: &'a Theme,
    out: String,
}

impl<'a> DiffFormatter<'a> {
    fn new(theme: &'a Theme) -> Self {
        let d = theme.dimmed;
        let out = format!("{d}```diff{d:#}\n");
        Self { theme, out }
    }

    fn append_removals(&mut self, text: &str) {
        let red = self.theme.tool_err;
        for line in text.lines().take(8) {
            self.out.push_str(&format!("{red}- {line}{red:#}\n"));
        }
        let count = text.lines().count();
        if count > 8 {
            let dim = self.theme.dimmed;
            self.out
                .push_str(&format!("{dim}... ({} more lines){dim:#}\n", count - 8));
        }
    }

    fn append_additions(&mut self, text: &str) {
        let green = self.theme.tool_ok;
        for line in text.lines().take(8) {
            self.out.push_str(&format!("{green}+ {line}{green:#}\n"));
        }
        let count = text.lines().count();
        if count > 8 {
            let dim = self.theme.dimmed;
            self.out
                .push_str(&format!("{dim}... ({} more lines){dim:#}\n", count - 8));
        }
    }

    fn format_entry(&mut self, idx: usize, edit: &serde_json::Value) {
        if idx > 0 {
            let dim = self.theme.dimmed;
            self.out.push_str(&format!("{dim}@@ edit #{} @@{dim:#}\n", idx + 1));
        }
        let old_text = edit.get("oldText").and_then(|v| v.as_str()).unwrap_or("");
        let new_text = edit.get("newText").and_then(|v| v.as_str()).unwrap_or("");
        self.append_removals(old_text);
        self.append_additions(new_text);
    }

    fn finish(mut self) -> String {
        let d = self.theme.dimmed;
        self.out.push_str(&format!("{d}```{d:#}\n"));
        self.out
    }
}

pub(super) fn format_edit_diff(args: &serde_json::Value, theme: &Theme) -> Option<String> {
    let edits = args.get("edits")?.as_array()?;
    if edits.is_empty() {
        return None;
    }
    let mut formatter = DiffFormatter::new(theme);
    for (idx, edit) in edits.iter().enumerate() {
        formatter.format_entry(idx, edit);
    }
    Some(formatter.finish())
}

pub(super) fn format_write_preview(args: &serde_json::Value, theme: &Theme) -> Option<String> {
    let content = args.get("content")?.as_str()?;
    if content.trim().is_empty() {
        return None;
    }
    let d = theme.dimmed;
    let green = theme.tool_ok;
    let mut out = format!("{d}```diff{d:#}\n");
    for line in content.lines().take(8) {
        out.push_str(&format!("{green}+ {line}{green:#}\n"));
    }
    let total = content.lines().count();
    if total > 8 {
        out.push_str(&format!("{d}... ({} more lines){d:#}\n", total - 8));
    }
    out.push_str(&format!("{d}```{d:#}\n"));
    Some(out)
}

pub(super) fn format_thinking_block(thinking_text: &str, theme: &Theme) -> String {
    let d = theme.dimmed;
    let mut out = String::new();
    for line in thinking_text.trim().lines() {
        out.push_str(&format!("{d}{line}{d:#}\n"));
    }
    out.push('\n');
    out
}
