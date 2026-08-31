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

pub fn wrap_to_width(content: &str, max_width: usize) -> Vec<String> {
    let max_width = max_width.max(1);
    let mut output = Vec::new();
    for line in content.split('\n') {
        if line.is_empty() {
            output.push(String::new());
            continue;
        }
        let mut current = String::new();
        let mut current_width = 0;
        let mut offset = 0;
        let mut active_ansi = String::new();

        while offset < line.len() {
            if line[offset..].starts_with('\x1b')
                && let Some(end) = line[offset..].find('m')
            {
                let seq = &line[offset..=offset + end];
                current.push_str(seq);
                if seq == "\x1b[0m" {
                    active_ansi.clear();
                } else {
                    active_ansi.push_str(seq);
                }
                offset += end + 1;
                continue;
            }
            let Some(c) = line[offset..].chars().next() else {
                break;
            };
            let char_w = UnicodeWidthChar::width(c).unwrap_or(0);
            if current_width > 0 && current_width + char_w > max_width {
                if !active_ansi.is_empty() {
                    current.push_str("\x1b[0m");
                }
                output.push(std::mem::take(&mut current));
                if !active_ansi.is_empty() {
                    current.push_str(&active_ansi);
                }
                current_width = 0;
            }
            current.push(c);
            current_width += char_w;
            offset += c.len_utf8();
        }
        output.push(current);
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
