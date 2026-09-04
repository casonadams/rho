//! Code fence state tracking for fenced markdown blocks.

use crate::ui::theme::Theme;

#[derive(Default)]
pub struct CodeFenceTracker {
    pub in_code_block: bool,
    pub code_lang: Option<String>,
}

impl CodeFenceTracker {
    pub fn toggle(&mut self, trimmed: &str, theme: &Theme) -> String {
        let tag = trimmed.trim_start_matches('`').trim();
        let dim = theme.dimmed;
        if self.in_code_block {
            self.in_code_block = false;
            self.code_lang = None;
            format!("{dim}```{dim:#}")
        } else {
            self.in_code_block = true;
            self.code_lang = (!tag.is_empty()).then(|| tag.to_string());
            format!("{dim}```{tag}{dim:#}")
        }
    }
}
