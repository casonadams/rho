use crate::ui::block::visible_width;
use crate::ui::wrap::{AnsiWordWrap, push_line_into};
use unicode_width::UnicodeWidthChar;

pub const SPINNER_FRAMES: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisualTruncateResult {
    pub visual_lines: Vec<String>,
    pub skipped_count: usize,
}

pub fn truncate_to_visual_lines(text: &str, max_visual_lines: usize, width: usize) -> VisualTruncateResult {
    if text.is_empty() {
        return VisualTruncateResult {
            visual_lines: Vec::new(),
            skipped_count: 0,
        };
    }
    let all_lines = wrap_to_width(text, width.max(1));
    if all_lines.len() <= max_visual_lines {
        return VisualTruncateResult {
            visual_lines: all_lines,
            skipped_count: 0,
        };
    }
    let skipped_count = all_lines.len() - max_visual_lines;
    let visual_lines = all_lines[skipped_count..].to_vec();
    VisualTruncateResult {
        visual_lines,
        skipped_count,
    }
}

struct LineWrapper<'a> {
    line: &'a str,
    offset: usize,
    current_line: String,
    ansi: AnsiWordWrap,
}

impl<'a> LineWrapper<'a> {
    fn new(line: &'a str, max_width: usize) -> Self {
        Self {
            line,
            offset: 0,
            current_line: String::new(),
            ansi: AnsiWordWrap::new(max_width),
        }
    }

    fn commit_pending_word(&mut self, output: &mut Vec<String>) {
        let keep_leading = output.is_empty();
        self.ansi
            .commit(&mut self.current_line, keep_leading, push_line_into(output));
    }

    fn handle_space(&mut self, c: char, output: &mut Vec<String>) {
        self.commit_pending_word(output);
        self.ansi.word.push_space(c, UnicodeWidthChar::width(c).unwrap_or(0));
    }

    fn handle_char(&mut self, c: char, cw: usize, output: &mut Vec<String>) {
        self.ansi
            .push_char(&mut self.current_line, c, cw, |line| output.push(std::mem::take(line)));
    }

    fn wrap(mut self, output: &mut Vec<String>) {
        while self.offset < self.line.len() {
            if let Some(len) = self.ansi.consume_ansi(&self.line[self.offset..]) {
                self.offset += len;
                continue;
            }
            let Some(c) = self.line[self.offset..].chars().next() else {
                break;
            };
            self.offset += c.len_utf8();
            if c == '\r' {
                continue;
            }

            if c == ' ' || c == '\t' {
                self.handle_space(c, output);
            } else {
                let cw = UnicodeWidthChar::width(c).unwrap_or(0);
                self.handle_char(c, cw, output);
            }
        }
        self.commit_pending_word(output);
        if visible_width(&self.current_line) > 0 || output.is_empty() {
            output.push(self.current_line);
        }
    }
}

pub fn wrap_to_width(content: &str, max_width: usize) -> Vec<String> {
    let max_width = max_width.max(1);
    let mut output = Vec::new();
    for line in content.split('\n') {
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() {
            output.push(String::new());
            continue;
        }
        LineWrapper::new(line, max_width).wrap(&mut output);
    }
    if output.is_empty() {
        output.push(String::new());
    }
    output
}
