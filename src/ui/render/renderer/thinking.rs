use crate::ui::theme::Theme;
use unicode_width::UnicodeWidthChar;

#[derive(Default)]
pub struct ThinkingStreamTracker {
    col: usize,
    pending_spaces: String,
    pending_spaces_width: usize,
    pending_word: String,
    pending_word_width: usize,
    at_line_start: bool,
}

impl ThinkingStreamTracker {
    pub fn new() -> Self {
        Self {
            col: 0,
            pending_spaces: String::new(),
            pending_spaces_width: 0,
            pending_word: String::new(),
            pending_word_width: 0,
            at_line_start: true,
        }
    }

    pub fn process_token(&mut self, token: &str, width: usize, theme: &Theme) -> String {
        let max_width = if width > 0 { width.saturating_sub(1).max(10) } else { 79 };
        let d = theme.dimmed;
        let mut out = String::new();

        for c in token.chars() {
            if c == '\n' {
                self.commit_pending_word(&mut out, max_width, d);
                out.push('\n');
                self.col = 0;
                self.at_line_start = true;
                self.pending_spaces.clear();
                self.pending_spaces_width = 0;
            } else if c == ' ' || c == '\t' {
                if !self.pending_word.is_empty() {
                    self.commit_pending_word(&mut out, max_width, d);
                }
                let cw = UnicodeWidthChar::width(c).unwrap_or(1);
                self.pending_spaces.push(c);
                self.pending_spaces_width += cw;
            } else {
                let cw = UnicodeWidthChar::width(c).unwrap_or(1);
                if self.pending_word_width + cw > max_width {
                    if !self.at_line_start && self.col > 1 {
                        out.push('\n');
                        out.push(' ');
                        self.col = 1;
                        self.pending_spaces.clear();
                        self.pending_spaces_width = 0;
                    } else if self.at_line_start && self.col == 0 {
                        out.push(' ');
                        self.col = 1;
                        self.at_line_start = false;
                    }
                    out.push_str(&format!("{d}{}{d:#}", self.pending_word));
                    out.push('\n');
                    out.push(' ');
                    self.col = 1;
                    self.pending_word.clear();
                    self.pending_word_width = 0;
                }
                self.pending_word.push(c);
                self.pending_word_width += cw;
            }
        }

        out
    }

    fn commit_pending_word(&mut self, out: &mut String, max_width: usize, d: anstyle::Style) {
        if self.pending_word.is_empty() && self.pending_word_width == 0 {
            return;
        }
        let needed = self.pending_spaces_width + self.pending_word_width;
        if !self.at_line_start && self.col + needed > max_width {
            out.push('\n');
            out.push(' ');
            self.col = 1;
            self.pending_spaces.clear();
            self.pending_spaces_width = 0;
        } else if self.at_line_start && self.col == 0 {
            out.push(' ');
            self.col = 1;
            self.at_line_start = false;
        }
        out.push_str(&format!("{d}{}{}{d:#}", self.pending_spaces, self.pending_word));
        self.col += self.pending_spaces_width + self.pending_word_width;
        self.pending_spaces.clear();
        self.pending_spaces_width = 0;
        self.pending_word.clear();
        self.pending_word_width = 0;
    }

    pub fn flush(&mut self, theme: &Theme) -> String {
        let mut out = String::new();
        let d = theme.dimmed;
        if !self.pending_word.is_empty() {
            if self.at_line_start && self.col == 0 {
                out.push(' ');
            }
            out.push_str(&format!("{d}{}{}{d:#}", self.pending_spaces, self.pending_word));
        }
        self.col = 0;
        self.pending_spaces.clear();
        self.pending_spaces_width = 0;
        self.pending_word.clear();
        self.pending_word_width = 0;
        self.at_line_start = true;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_thinking_wraps_on_word_boundaries() {
        let theme = Theme::default();
        let mut tracker = ThinkingStreamTracker::new();

        let mut output = String::new();
        output.push_str(&tracker.process_token("alpha beta ", 16, &theme));
        output.push_str(&tracker.process_token("gamma delta ", 16, &theme));
        output.push_str(&tracker.process_token("epsilon", 16, &theme));
        output.push_str(&tracker.flush(&theme));

        let lines: Vec<&str> = output.lines().collect();
        assert!(lines.len() >= 2);
        assert!(lines[0].contains("alpha"));
        assert!(lines[0].contains("beta"));
        assert!(lines[1].contains("gamma"));
        assert!(lines[1].contains("delta"));
    }

    #[test]
    fn stream_thinking_preserves_single_space_indent_on_wrapped_lines() {
        let theme = Theme::default();
        let mut tracker = ThinkingStreamTracker::new();

        let mut output = String::new();
        output.push_str(&tracker.process_token("first second third ", 12, &theme));
        output.push_str(&tracker.process_token("fourth", 12, &theme));
        output.push_str(&tracker.flush(&theme));

        for line in output.lines() {
            assert!(
                line.starts_with(' ') || line.starts_with("\x1b"),
                "each line must be indented with space: {line:?}"
            );
        }
    }
}
