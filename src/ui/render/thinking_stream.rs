use unicode_width::UnicodeWidthChar;

use crate::ui::theme::Theme;

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
            } else if c == '\r' {
                continue;
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

    fn strip_ansi(s: &str) -> String {
        crate::ui::block::strip_ansi(s)
    }

    #[test]
    fn stream_thinking_wraps_on_word_boundaries() {
        let mut tracker = ThinkingStreamTracker::new();
        let theme = Theme::default();
        let mut out = String::new();
        out.push_str(&tracker.process_token("The quick brown fox jumps over ", 20, &theme));
        out.push_str(&tracker.flush(&theme));
        let lines: Vec<&str> = out.lines().collect();
        assert!(lines.len() >= 2);
        assert!(!out.contains("qui-\nck"));
    }

    #[test]
    fn stream_thinking_preserves_single_space_indent_on_wrapped_lines() {
        let mut tracker = ThinkingStreamTracker::new();
        let theme = Theme::default();
        let mut out = String::new();
        out.push_str(&tracker.process_token("alpha beta gamma delta epsilon zeta", 15, &theme));
        out.push_str(&tracker.flush(&theme));
        let lines: Vec<&str> = out.lines().collect();
        for line in &lines {
            let stripped = strip_ansi(line);
            assert!(stripped.starts_with(' '));
        }
    }

    #[test]
    fn stream_thinking_preserves_natural_line_breaks() {
        let mut tracker = ThinkingStreamTracker::new();
        let theme = Theme::default();
        let mut out = String::new();
        out.push_str(&tracker.process_token("line one\nline two", 80, &theme));
        out.push_str(&tracker.flush(&theme));
        let stripped = strip_ansi(&out);
        assert!(stripped.contains("line one\n"));
        assert!(stripped.contains("line two"));
    }

    #[test]
    fn stream_thinking_preserves_token_split_natural_line_breaks() {
        let mut tracker = ThinkingStreamTracker::new();
        let theme = Theme::default();
        let mut out = String::new();
        out.push_str(&tracker.process_token("step 1", 80, &theme));
        out.push_str(&tracker.process_token("\n", 80, &theme));
        out.push_str(&tracker.process_token("step 2", 80, &theme));
        out.push_str(&tracker.flush(&theme));
        let stripped = strip_ansi(&out);
        assert!(stripped.contains("step 1\n"));
        assert!(stripped.contains("step 2"));
    }

    #[test]
    fn stream_thinking_preserves_natural_paragraph_breaks() {
        let mut tracker = ThinkingStreamTracker::new();
        let theme = Theme::default();
        let mut out = String::new();
        out.push_str(&tracker.process_token("para one\n\npara two", 80, &theme));
        out.push_str(&tracker.flush(&theme));
        let stripped = strip_ansi(&out);
        assert!(stripped.contains("para one\n\n"));
        assert!(stripped.contains("para two"));
    }

    #[test]
    fn stream_thinking_handles_crlf_line_breaks() {
        let mut tracker = ThinkingStreamTracker::new();
        let theme = Theme::default();
        let mut out = String::new();
        out.push_str(&tracker.process_token("line one\r\nline two", 80, &theme));
        out.push_str(&tracker.flush(&theme));
        let stripped = strip_ansi(&out);
        assert!(!stripped.contains('\r'));
        assert!(stripped.contains("line one\n"));
        assert!(stripped.contains("line two"));
    }
}
