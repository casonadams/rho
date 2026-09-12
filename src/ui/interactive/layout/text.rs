use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

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

pub fn visible_width(content: &str) -> usize {
    let clean = crate::ui::block::ANSI_PATTERN.replace_all(content, "");
    UnicodeWidthStr::width(clean.as_ref())
}

struct LineWrapper<'a> {
    line: &'a str,
    offset: usize,
    max_width: usize,
    current_line: String,
    current_width: usize,
    active_ansi: String,
    pending_spaces: String,
    pending_spaces_width: usize,
    pending_word: String,
    pending_word_width: usize,
}

impl<'a> LineWrapper<'a> {
    fn new(line: &'a str, max_width: usize) -> Self {
        Self {
            line,
            offset: 0,
            max_width,
            current_line: String::new(),
            current_width: 0,
            active_ansi: String::new(),
            pending_spaces: String::new(),
            pending_spaces_width: 0,
            pending_word: String::new(),
            pending_word_width: 0,
        }
    }

    fn try_consume_ansi(&mut self) -> bool {
        if self.line[self.offset..].starts_with('\x1b')
            && let Some(end) = self.line[self.offset..].find('m')
        {
            let seq = &self.line[self.offset..=self.offset + end];
            if seq == "\x1b[0m" || seq == "\x1b[m" {
                self.active_ansi.clear();
            } else {
                self.active_ansi.push_str(seq);
            }
            self.pending_word.push_str(seq);
            self.offset += end + 1;
            return true;
        }
        false
    }

    fn flush_current_line(&mut self, output: &mut Vec<String>) {
        if !self.active_ansi.is_empty() {
            self.current_line.push_str("\x1b[0m");
        }
        output.push(std::mem::take(&mut self.current_line));
        if !self.active_ansi.is_empty() {
            self.current_line.push_str(&self.active_ansi);
        }
        self.current_width = 0;
    }

    fn commit_pending_word(&mut self, output: &mut Vec<String>) {
        if self.pending_word.is_empty() && self.pending_word_width == 0 {
            return;
        }
        let needed = self.pending_spaces_width + self.pending_word_width;
        if self.current_width > 0 && self.current_width + needed > self.max_width {
            self.flush_current_line(output);
            self.pending_spaces.clear();
            self.pending_spaces_width = 0;
        }
        if self.current_width > 0 || output.is_empty() {
            self.current_line.push_str(&self.pending_spaces);
            self.current_width += self.pending_spaces_width;
        }
        self.pending_spaces.clear();
        self.pending_spaces_width = 0;

        self.current_line.push_str(&self.pending_word);
        self.current_width += self.pending_word_width;
        self.pending_word.clear();
        self.pending_word_width = 0;
    }

    fn wrap(mut self, output: &mut Vec<String>) {
        while self.offset < self.line.len() {
            if self.try_consume_ansi() {
                continue;
            }
            let Some(c) = self.line[self.offset..].chars().next() else {
                break;
            };
            self.offset += c.len_utf8();

            if c == ' ' || c == '\t' {
                self.commit_pending_word(output);
                let cw = UnicodeWidthChar::width(c).unwrap_or(0);
                self.pending_spaces.push(c);
                self.pending_spaces_width += cw;
            } else {
                let cw = UnicodeWidthChar::width(c).unwrap_or(0);
                if self.pending_word_width + cw > self.max_width {
                    if self.current_width > 0 {
                        self.flush_current_line(output);
                        self.pending_spaces.clear();
                        self.pending_spaces_width = 0;
                    }
                    if self.pending_word_width + cw > self.max_width && self.pending_word_width > 0 {
                        self.current_line.push_str(&self.pending_word);
                        self.flush_current_line(output);
                        self.pending_word.clear();
                        self.pending_word_width = 0;
                    }
                }
                self.pending_word.push(c);
                self.pending_word_width += cw;
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

pub fn wrap_words_to_width(content: &str, max_width: usize) -> Vec<String> {
    wrap_to_width(content, max_width)
}

pub(crate) fn truncate_to_width(value: &str, width: usize) -> String {
    let mut current_width = 0;
    let mut truncated = String::new();
    for character in value.chars() {
        let character_width = character.width().unwrap_or(0);
        if current_width + character_width > width {
            break;
        }
        truncated.push(character);
        current_width += character_width;
    }
    truncated
}
