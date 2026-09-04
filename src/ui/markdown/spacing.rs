//! Output spacing tracker ensuring single-blank-line separation between blocks.

#[derive(Debug)]
pub struct SpacingTracker {
    is_start: bool,
    last_line_was_blank: bool,
}

impl Default for SpacingTracker {
    fn default() -> Self {
        Self {
            is_start: true,
            last_line_was_blank: true,
        }
    }
}

impl SpacingTracker {
    pub fn note_content(&mut self) {
        self.is_start = false;
        self.last_line_was_blank = false;
    }

    pub fn note_blank(&mut self) {
        self.last_line_was_blank = true;
    }

    pub fn append_block(&mut self, out: &mut String, rendered: &str) {
        if !self.is_start && !self.last_line_was_blank {
            out.push('\n');
        }
        out.push_str(rendered);
        self.note_content();
    }

    pub fn ensure_preceding_blank(&mut self, out: &mut String) {
        if !self.is_start && !self.last_line_was_blank {
            out.push('\n');
        }
    }

    pub fn handle_empty_line(&mut self, out: &mut String) {
        if !self.is_start && !self.last_line_was_blank {
            out.push('\n');
            self.note_blank();
        }
    }
}
