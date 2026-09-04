//! Mermaid diagram block collection and toggling.

use super::elements::render_mermaid_block;
use crate::ui::theme::Theme;

#[derive(Default)]
pub struct MermaidBlockTracker {
    in_block: bool,
    lines: Vec<String>,
}

impl MermaidBlockTracker {
    pub fn in_block(&self) -> bool {
        self.in_block
    }

    pub fn push_line(&mut self, line: &str) {
        self.lines.push(line.to_string());
    }

    pub fn try_render_fence(&mut self, trimmed: &str, theme: &Theme) -> Option<Option<String>> {
        if !trimmed.starts_with("```") {
            return None;
        }
        let tag = trimmed.trim_start_matches('`').trim();
        if self.in_block {
            self.in_block = false;
            let src = std::mem::take(&mut self.lines).join("\n");
            Some(Some(render_mermaid_block(&src, theme)))
        } else if tag.eq_ignore_ascii_case("mermaid") {
            self.in_block = true;
            self.lines.clear();
            Some(None)
        } else {
            None
        }
    }

    pub fn flush_rendered(&mut self, theme: &Theme) -> Option<String> {
        if self.in_block && !self.lines.is_empty() {
            self.in_block = false;
            Some(render_mermaid_block(&std::mem::take(&mut self.lines).join("\n"), theme))
        } else {
            self.in_block = false;
            None
        }
    }
}
