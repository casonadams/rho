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
    current: String,
    current_width: usize,
    active_ansi: String,
}

impl<'a> LineWrapper<'a> {
    fn new(line: &'a str, max_width: usize) -> Self {
        Self {
            line,
            offset: 0,
            max_width,
            current: String::new(),
            current_width: 0,
            active_ansi: String::new(),
        }
    }

    fn try_consume_ansi(&mut self) -> bool {
        if self.line[self.offset..].starts_with('\x1b')
            && let Some(end) = self.line[self.offset..].find('m')
        {
            let seq = &self.line[self.offset..=self.offset + end];
            self.current.push_str(seq);
            if seq == "\x1b[0m" {
                self.active_ansi.clear();
            } else {
                self.active_ansi.push_str(seq);
            }
            self.offset += end + 1;
            return true;
        }
        false
    }

    fn push_wrapped_char(&mut self, output: &mut Vec<String>, c: char) {
        let char_w = UnicodeWidthChar::width(c).unwrap_or(0);
        if self.current_width > 0 && self.current_width + char_w > self.max_width {
            if !self.active_ansi.is_empty() {
                self.current.push_str("\x1b[0m");
            }
            output.push(std::mem::take(&mut self.current));
            if !self.active_ansi.is_empty() {
                self.current.push_str(&self.active_ansi);
            }
            self.current_width = 0;
        }
        self.current.push(c);
        self.current_width += char_w;
        self.offset += c.len_utf8();
    }

    fn wrap(mut self, output: &mut Vec<String>) {
        while self.offset < self.line.len() {
            if self.try_consume_ansi() {
                continue;
            }
            let Some(c) = self.line[self.offset..].chars().next() else {
                break;
            };
            self.push_wrapped_char(output, c);
        }
        output.push(self.current);
    }
}

pub fn wrap_to_width(content: &str, max_width: usize) -> Vec<String> {
    let max_width = max_width.max(1);
    let mut output = Vec::new();
    for line in content.split('\n') {
        if line.is_empty() {
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
    let max_width = max_width.max(1);
    let mut output = Vec::new();
    for line in content.split('\n') {
        if line.trim().is_empty() {
            output.push(String::new());
            continue;
        }
        let mut current_line = String::new();
        let mut current_width = 0;
        for word in line.split_whitespace() {
            let word_width = visible_width(word);
            let space_width = usize::from(current_width > 0);
            if current_width + space_width + word_width <= max_width {
                if space_width > 0 {
                    current_line.push(' ');
                }
                current_line.push_str(word);
                current_width += space_width + word_width;
            } else {
                if current_width > 0 {
                    output.push(current_line);
                    current_line = String::new();
                    current_width = 0;
                }
                if word_width <= max_width {
                    current_line.push_str(word);
                    current_width = word_width;
                } else {
                    let chunks = wrap_to_width(word, max_width);
                    for (i, chunk) in chunks.iter().enumerate() {
                        if i + 1 < chunks.len() {
                            output.push(chunk.clone());
                        } else {
                            current_line = chunk.clone();
                            current_width = visible_width(&current_line);
                        }
                    }
                }
            }
        }
        if !current_line.is_empty() {
            output.push(current_line);
        }
    }
    if output.is_empty() {
        output.push(String::new());
    }
    output
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
