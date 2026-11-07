use crate::ui::theme::Theme;
use crate::ui::wrap::AnsiWordWrap;
use unicode_width::UnicodeWidthChar;

pub struct StreamWordWrapper {
    ansi: AnsiWordWrap,
}

impl Default for StreamWordWrapper {
    fn default() -> Self {
        Self::new(0)
    }
}

fn push_newline(line: &mut String) {
    line.push('\n');
}

impl StreamWordWrapper {
    pub fn new(max_width: usize) -> Self {
        Self {
            ansi: AnsiWordWrap::new(max_width),
        }
    }

    pub fn set_width(&mut self, width: usize) {
        self.ansi.word.width = width;
    }

    fn commit_pending_word(&mut self, out: &mut String) {
        self.ansi.commit(out, true, push_newline);
    }

    fn handle_newline(&mut self, out: &mut String) {
        self.commit_pending_word(out);
        out.push('\n');
        self.ansi.word.current_width = 0;
        self.ansi.word.clear_spaces();
    }

    fn handle_whitespace(&mut self, c: char, out: &mut String) {
        self.commit_pending_word(out);
        let cw = UnicodeWidthChar::width(c).unwrap_or(1);
        self.ansi.word.push_space(c, cw);
    }

    pub fn process_chunk(&mut self, chunk: &str) -> String {
        if self.ansi.word.width == 0 {
            return chunk.to_string();
        }
        let mut out = String::new();
        let mut offset = 0;

        while offset < chunk.len() {
            if let Some(len) = self.ansi.consume_ansi(&chunk[offset..]) {
                offset += len;
                continue;
            }

            let Some(c) = chunk[offset..].chars().next() else {
                break;
            };
            offset += c.len_utf8();

            if c == '\n' {
                self.handle_newline(&mut out);
            } else if c == '\r' {
                continue;
            } else if c == ' ' || c == '\t' {
                self.handle_whitespace(c, &mut out);
            } else {
                let cw = UnicodeWidthChar::width(c).unwrap_or(1);
                self.ansi.push_char(&mut out, c, cw, push_newline);
            }
        }
        out
    }

    pub fn flush(&mut self) -> String {
        let mut out = String::new();
        self.commit_pending_word(&mut out);
        self.ansi.word.current_width = 0;
        self.ansi.word.clear_spaces();
        self.ansi.word.pending_word.clear();
        self.ansi.word.pending_word_width = 0;
        self.ansi.clear_ansi();
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

    #[test]
    fn handle_newline_commits_the_word_and_drops_trailing_spaces() {
        let cases = [
            ("hello\nworld", "hello\nworld"),
            ("ab  \ncd", "ab\ncd"),
            ("ab\n", "ab\n"),
            ("\n\n", "\n\n"),
        ];
        for (input, expected) in cases {
            let mut wrapper = StreamWordWrapper::new(20);
            let mut output = wrapper.process_chunk(input);
            output.push_str(&wrapper.flush());
            assert_eq!(output, expected, "input {input:?}");
        }
    }

    #[test]
    fn flush_resets_wrap_state_before_the_next_segment() {
        let cases = [
            ("\x1b[1mab", "cdefghij", "cdefg\nhij"),
            ("ab ", "cd", "cd"),
            ("abc", "de", "de"),
        ];
        for (first, second, expected) in cases {
            let mut wrapper = StreamWordWrapper::new(5);
            wrapper.process_chunk(first);
            wrapper.flush();
            let mut output = wrapper.process_chunk(second);
            output.push_str(&wrapper.flush());
            assert_eq!(output, expected, "segment {second:?}");
        }
    }
}
