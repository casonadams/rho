use crate::ui::theme::Theme;
use unicode_width::UnicodeWidthChar;

pub struct StreamWordWrapper {
    max_width: usize,
    col: usize,
    active_ansi: String,
    pending_ansi: String,
    pending_spaces: String,
    pending_spaces_width: usize,
    pending_word: String,
    pending_word_width: usize,
}

impl Default for StreamWordWrapper {
    fn default() -> Self {
        Self::new(0)
    }
}

impl StreamWordWrapper {
    pub fn new(max_width: usize) -> Self {
        Self {
            max_width,
            col: 0,
            active_ansi: String::new(),
            pending_ansi: String::new(),
            pending_spaces: String::new(),
            pending_spaces_width: 0,
            pending_word: String::new(),
            pending_word_width: 0,
        }
    }

    pub fn set_width(&mut self, width: usize) {
        self.max_width = width;
    }

    pub fn process_chunk(&mut self, chunk: &str) -> String {
        if self.max_width == 0 {
            return chunk.to_string();
        }
        let mut out = String::new();
        let mut offset = 0;
        let len = chunk.len();

        while offset < len {
            if chunk[offset..].starts_with('\x1b')
                && let Some(end) = chunk[offset..].find('m')
            {
                let seq = &chunk[offset..=offset + end];
                self.pending_ansi.push_str(seq);
                self.pending_word.push_str(seq);
                offset += end + 1;
                continue;
            }

            let Some(c) = chunk[offset..].chars().next() else {
                break;
            };
            offset += c.len_utf8();

            if c == '\n' {
                self.commit_pending_word(&mut out);
                out.push('\n');
                self.col = 0;
                self.pending_spaces.clear();
                self.pending_spaces_width = 0;
            } else if c == '\r' {
                continue;
            } else if c == ' ' || c == '\t' {
                if !self.pending_word.is_empty() {
                    self.commit_pending_word(&mut out);
                }
                let cw = UnicodeWidthChar::width(c).unwrap_or(1);
                self.pending_spaces.push(c);
                self.pending_spaces_width += cw;
            } else {
                let cw = UnicodeWidthChar::width(c).unwrap_or(1);
                if self.pending_word_width + cw > self.max_width {
                    if self.col > 0 {
                        self.flush_line(&mut out);
                        self.pending_spaces.clear();
                        self.pending_spaces_width = 0;
                    }
                    if self.pending_word_width > 0 && self.pending_word_width + cw > self.max_width {
                        out.push_str(&self.pending_word);
                        self.apply_pending_ansi();
                        self.flush_line(&mut out);
                        self.pending_word.clear();
                        self.pending_word_width = 0;
                    }
                }
                self.pending_word.push(c);
                self.pending_word_width += cw;
            }
        }
        out
    }

    fn flush_line(&mut self, out: &mut String) {
        if !self.active_ansi.is_empty() {
            out.push_str("\x1b[0m");
        }
        out.push('\n');
        if !self.active_ansi.is_empty() {
            out.push_str(&self.active_ansi);
        }
        self.col = 0;
    }

    fn commit_pending_word(&mut self, out: &mut String) {
        if self.pending_word.is_empty() && self.pending_word_width == 0 {
            return;
        }
        let needed = self.pending_spaces_width + self.pending_word_width;
        if self.col > 0 && self.col + needed > self.max_width {
            self.flush_line(out);
            self.pending_spaces.clear();
            self.pending_spaces_width = 0;
        }
        if self.col > 0 || !self.pending_spaces.is_empty() {
            out.push_str(&self.pending_spaces);
            self.col += self.pending_spaces_width;
        }
        self.pending_spaces.clear();
        self.pending_spaces_width = 0;

        out.push_str(&self.pending_word);
        self.col += self.pending_word_width;
        self.pending_word.clear();
        self.pending_word_width = 0;
        self.apply_pending_ansi();
    }

    fn apply_pending_ansi(&mut self) {
        if self.pending_ansi.is_empty() {
            return;
        }
        let mut rem = self.pending_ansi.as_str();
        while let Some(start) = rem.find('\x1b') {
            if let Some(end) = rem[start..].find('m') {
                let seq = &rem[start..=start + end];
                if seq == "\x1b[0m" || seq == "\x1b[m" {
                    self.active_ansi.clear();
                } else {
                    self.active_ansi.push_str(seq);
                }
                rem = &rem[start + end + 1..];
            } else {
                break;
            }
        }
        self.pending_ansi.clear();
    }

    pub fn flush(&mut self) -> String {
        let mut out = String::new();
        self.commit_pending_word(&mut out);
        self.col = 0;
        self.pending_spaces.clear();
        self.pending_spaces_width = 0;
        self.pending_word.clear();
        self.pending_word_width = 0;
        self.pending_ansi.clear();
        self.active_ansi.clear();
        out
    }
}

#[derive(Default)]
pub struct InlineStreamTracker {
    in_bold: bool,
    in_italic: bool,
    in_code: bool,
    pending_star: bool,
    scratch: Vec<char>,
}

impl InlineStreamTracker {
    pub fn reset_line(&mut self) -> String {
        let mut out = String::new();
        if self.pending_star {
            out.push('*');
            self.pending_star = false;
        }
        if self.in_bold {
            out.push_str(&anstyle::Style::new().bold().render_reset().to_string());
            self.in_bold = false;
        }
        if self.in_italic {
            out.push_str(&anstyle::Style::new().italic().render_reset().to_string());
            self.in_italic = false;
        }
        if self.in_code {
            out.push_str(&anstyle::Style::new().render_reset().to_string());
            self.in_code = false;
        }
        out
    }

    fn toggle_bold(&mut self, out: &mut String) {
        let bold_style = anstyle::Style::new().bold();
        if self.in_bold {
            out.push_str(&bold_style.render_reset().to_string());
            self.in_bold = false;
        } else {
            out.push_str(&bold_style.render().to_string());
            self.in_bold = true;
        }
    }

    fn toggle_italic(&mut self, out: &mut String) {
        let italic_style = anstyle::Style::new().italic();
        if self.in_italic {
            out.push_str(&italic_style.render_reset().to_string());
            self.in_italic = false;
        } else {
            out.push_str(&italic_style.render().to_string());
            self.in_italic = true;
        }
    }

    fn toggle_code(&mut self, out: &mut String, theme: &Theme) {
        if self.in_code {
            out.push_str(&theme.code_inline.render_reset().to_string());
            self.in_code = false;
        } else {
            out.push_str(&theme.code_inline.render().to_string());
            self.in_code = true;
        }
    }

    fn handle_pending_star(&mut self, first: char, out: &mut String) -> usize {
        self.pending_star = false;
        if first == '*' {
            self.toggle_bold(out);
            1
        } else if first.is_whitespace() {
            out.push('*');
            0
        } else {
            self.toggle_italic(out);
            0
        }
    }

    fn handle_star(&mut self, chars: &[char], i: usize, out: &mut String) -> usize {
        if i + 1 == chars.len() {
            self.pending_star = true;
            return 1;
        }
        if self.in_italic {
            if i > 0 && chars[i - 1].is_whitespace() {
                out.push('*');
            } else {
                self.toggle_italic(out);
            }
        } else if chars[i + 1].is_whitespace() {
            out.push('*');
        } else {
            self.toggle_italic(out);
        }
        1
    }

    fn process_token_char(&mut self, chars: &[char], i: usize, out: &mut String, theme: &Theme) -> usize {
        if chars[i] == '`' {
            self.toggle_code(out, theme);
            1
        } else if self.in_code {
            out.push(chars[i]);
            1
        } else if i + 1 < chars.len() && chars[i] == '*' && chars[i + 1] == '*' {
            self.toggle_bold(out);
            2
        } else if chars[i] == '*' {
            self.handle_star(chars, i, out)
        } else {
            out.push(chars[i]);
            1
        }
    }

    pub fn render_inline_token(&mut self, token: &str, theme: &Theme) -> String {
        let mut out = String::new();
        let mut chars = std::mem::take(&mut self.scratch);
        chars.clear();
        chars.extend(token.chars());
        let len = chars.len();
        let mut i = 0;

        if self.pending_star && len > 0 {
            i = self.handle_pending_star(chars[0], &mut out);
        }

        while i < len {
            i += self.process_token_char(&chars, i, &mut out, theme);
        }

        self.scratch = chars;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_word_wrapper_wraps_across_chunks() {
        let mut wrapper = StreamWordWrapper::new(12);
        let mut output = String::new();
        output.push_str(&wrapper.process_chunk("alpha "));
        output.push_str(&wrapper.process_chunk("beta "));
        output.push_str(&wrapper.process_chunk("gamma "));
        output.push_str(&wrapper.process_chunk("delta"));
        output.push_str(&wrapper.flush());

        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "alpha beta");
        assert_eq!(lines[1], "gamma delta");
    }

    #[test]
    fn stream_word_wrapper_preserves_ansi_across_wrap() {
        let mut wrapper = StreamWordWrapper::new(18);
        let mut output = String::new();
        output.push_str(&wrapper.process_chunk("normal "));
        output.push_str(&wrapper.process_chunk("\x1b[1mbold_one "));
        output.push_str(&wrapper.process_chunk("bold_two\x1b[0m"));
        output.push_str(&wrapper.flush());

        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("normal"));
        assert!(lines[0].contains("\x1b[1mbold_one"));
        assert!(lines[1].starts_with("\x1b[1m"));
        assert!(lines[1].contains("bold_two\x1b[0m"));
    }

    #[test]
    fn stream_word_wrapper_unconstrained_when_zero() {
        let mut wrapper = StreamWordWrapper::new(0);
        let mut output = String::new();
        output.push_str(&wrapper.process_chunk("this is a long unconstrained chunk of text that never wraps"));
        output.push_str(&wrapper.flush());

        assert_eq!(output, "this is a long unconstrained chunk of text that never wraps");
    }

    #[test]
    fn stream_word_wrapper_breaks_long_unbroken_word() {
        let mut wrapper = StreamWordWrapper::new(8);
        let mut output = String::new();
        output.push_str(&wrapper.process_chunk("supercalifragilistic"));
        output.push_str(&wrapper.flush());

        let lines: Vec<&str> = output.lines().collect();
        assert!(lines.len() >= 2);
        for line in &lines {
            assert!(line.len() <= 8);
        }
    }
}
