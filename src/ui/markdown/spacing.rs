//! Output spacing tracker ensuring single-blank-line separation between blocks.

#[derive(Debug)]
pub struct SpacingTracker {
    is_start: bool,
    pending_blank: bool,
}

impl Default for SpacingTracker {
    fn default() -> Self {
        Self {
            is_start: true,
            pending_blank: false,
        }
    }
}

impl SpacingTracker {
    pub fn note_content(&mut self) {
        self.is_start = false;
        self.pending_blank = false;
    }

    pub fn note_blank(&mut self) {
        if !self.is_start {
            self.pending_blank = true;
        }
    }

    pub fn prepare_content(&mut self, out: &mut String) {
        if !self.is_start && self.pending_blank {
            out.push('\n');
        }
        self.is_start = false;
        self.pending_blank = false;
    }

    pub fn append_block(&mut self, out: &mut String, rendered: &str) {
        self.prepare_content(out);
        out.push_str(rendered);
        self.note_content();
    }

    pub fn ensure_preceding_blank(&mut self, _out: &mut String) {
        self.note_blank();
    }

    pub fn handle_empty_line(&mut self, _out: &mut String) {
        self.note_blank();
    }
}
