use anstyle::Style;
use regex::Regex;
use std::sync::LazyLock;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub static ANSI_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\x1b\[[0-9;]*m").expect("valid ANSI escape pattern"));

pub fn strip_ansi(content: &str) -> String {
    strip_ansi_escapes::strip_str(content)
}

pub fn visible_width(content: &str) -> usize {
    let clean = strip_ansi_escapes::strip_str(content);
    UnicodeWidthStr::width(clean.replace('\r', "").as_str())
}

fn skip_color_params(params: &mut std::iter::Peekable<std::str::Split<'_, char>>) {
    match params.peek().copied() {
        Some("5") => {
            params.next();
            params.next();
        }
        Some("2") => {
            params.next();
            params.next();
            params.next();
            params.next();
        }
        _ => {}
    }
}

fn sgr_resets_background(sequence: &str) -> bool {
    let Some(inner) = sequence.strip_prefix("\x1b[").and_then(|s| s.strip_suffix('m')) else {
        return false;
    };
    if inner.is_empty() {
        return true;
    }
    let mut params = inner.split(';').peekable();
    while let Some(param) = params.next() {
        if param.is_empty() || param == "0" || param == "00" || param == "49" {
            return true;
        }
        if param == "38" || param == "48" {
            skip_color_params(&mut params);
        }
    }
    false
}

struct WrapState<'a> {
    lines: &'a mut Vec<String>,
    width: usize,
    bg_code: String,
    current: String,
    active_sgr: String,
    current_width: usize,
    offset: usize,
    pending_spaces: String,
    pending_spaces_width: usize,
    pending_word: String,
    pending_word_width: usize,
}

impl WrapState<'_> {
    fn flush_line(&mut self) {
        self.lines.push(std::mem::take(&mut self.current));
        self.current.push_str(&self.active_sgr);
        self.current_width = 0;
    }

    fn commit_pending_word(&mut self) {
        if self.pending_word.is_empty() && self.pending_word_width == 0 {
            return;
        }
        let needed = self.pending_spaces_width + self.pending_word_width;
        if self.current_width > 0 && self.current_width + needed > self.width {
            self.flush_line();
            self.pending_spaces.clear();
            self.pending_spaces_width = 0;
        }
        if self.current_width > 0 || self.lines.is_empty() {
            self.current.push_str(&self.pending_spaces);
            self.current_width += self.pending_spaces_width;
        }
        self.pending_spaces.clear();
        self.pending_spaces_width = 0;

        self.current.push_str(&self.pending_word);
        self.current_width += self.pending_word_width;
        self.pending_word.clear();
        self.pending_word_width = 0;
    }

    fn push_char(&mut self, character: char, character_width: usize) {
        if character == ' ' || character == '\t' {
            self.commit_pending_word();
            self.pending_spaces.push(character);
            self.pending_spaces_width += character_width;
        } else {
            let needed = self.pending_spaces_width + self.pending_word_width + character_width;
            if self.current_width > 0 && self.current_width + needed > self.width {
                self.flush_line();
                self.pending_spaces.clear();
                self.pending_spaces_width = 0;
            }
            if self.pending_word_width + character_width > self.width && self.pending_word_width > 0 {
                self.current.push_str(&self.pending_word);
                self.flush_line();
                self.pending_word.clear();
                self.pending_word_width = 0;
            }
            self.pending_word.push(character);
            self.pending_word_width += character_width;
        }
    }

    fn process_sgr(&mut self, matched: &str) {
        self.pending_word.push_str(matched);
        if matched == "\x1b[0m" || matched == "\x1b[m" {
            self.active_sgr.clear();
            if !self.bg_code.is_empty() {
                self.pending_word.push_str(&self.bg_code);
            }
        } else {
            if sgr_resets_background(matched) && !self.bg_code.is_empty() {
                self.pending_word.push_str(&self.bg_code);
            }
            self.active_sgr.push_str(matched);
        }
        self.offset += matched.len();
    }
}

pub(crate) fn wrap_styled_line(line: &str, width: usize, base_style: Style) -> Vec<String> {
    if width == 0 {
        return vec![String::new()];
    }
    let bg_code = base_style.render().to_string();
    let mut lines = Vec::new();
    let mut state = WrapState {
        lines: &mut lines,
        width,
        bg_code,
        current: String::new(),
        active_sgr: String::new(),
        current_width: 0,
        offset: 0,
        pending_spaces: String::new(),
        pending_spaces_width: 0,
        pending_word: String::new(),
        pending_word_width: 0,
    };

    while state.offset < line.len() {
        if let Some(matched) = ANSI_PATTERN.find(&line[state.offset..])
            && matched.start() == 0
        {
            state.process_sgr(matched.as_str());
            continue;
        }
        let Some(character) = line[state.offset..].chars().next() else {
            break;
        };
        state.offset += character.len_utf8();
        if character == '\r' {
            continue;
        }
        let character_width = UnicodeWidthChar::width(character).unwrap_or(0);
        state.push_char(character, character_width);
    }
    state.commit_pending_word();
    if !state.current.is_empty() || state.lines.is_empty() {
        state.lines.push(state.current);
    }
    lines
}

pub(crate) fn wrap_plain_text(content: &str, width: usize) -> Vec<String> {
    content
        .lines()
        .flat_map(|line| wrap_styled_line(line, width, Style::new()))
        .collect()
}

const HORIZONTAL_PADDING: usize = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BlockMode {
    #[default]
    Fill,
    Border,
}

pub struct BlockFormat {
    style: Style,
    width: usize,
    vertical_padding: bool,
    mode: BlockMode,
    border_style: Style,
}

impl BlockFormat {
    pub fn new(style: Style, width: usize) -> Self {
        Self {
            style,
            width,
            vertical_padding: false,
            mode: BlockMode::Fill,
            border_style: Style::new(),
        }
    }

    pub fn border(border_style: Style, width: usize) -> Self {
        Self {
            style: Style::new(),
            width,
            vertical_padding: false,
            mode: BlockMode::Border,
            border_style,
        }
    }

    pub fn with_mode(mut self, mode: BlockMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn with_border_style(mut self, border_style: Style) -> Self {
        self.border_style = border_style;
        self
    }

    pub fn with_vertical_padding(mut self) -> Self {
        self.vertical_padding = true;
        self
    }

    pub fn render_plain(&self, content: &str) -> String {
        let inner_width = self.inner_width();
        let lines = wrap_plain_text(content, inner_width);
        self.render_lines(&lines)
    }

    pub fn render_styled(&self, content: &str) -> String {
        let inner_width = self.inner_width();
        let wrap_style = match self.mode {
            BlockMode::Fill => self.style,
            BlockMode::Border => Style::new(),
        };
        let lines: Vec<String> = content
            .lines()
            .flat_map(|line| wrap_styled_line(line, inner_width, wrap_style))
            .collect();
        self.render_lines(&lines)
    }

    pub fn render_line(&self, content: &str) -> String {
        let inner_width = self.inner_width();
        let wrap_style = match self.mode {
            BlockMode::Fill => self.style,
            BlockMode::Border => Style::new(),
        };
        let lines = wrap_styled_line(content, inner_width, wrap_style);
        let mut rendered = self.render_lines(&lines);
        rendered.pop();
        rendered
    }

    pub fn inner_width(&self) -> usize {
        match self.mode {
            BlockMode::Fill => self.width.saturating_sub(HORIZONTAL_PADDING * 2).max(1),
            BlockMode::Border => self.width.saturating_sub(4).max(1),
        }
    }

    fn render_lines(&self, lines: &[String]) -> String {
        match self.mode {
            BlockMode::Fill => {
                let mut output = String::new();
                if self.vertical_padding {
                    output.push_str(&self.padded_line(""));
                }
                for line in lines {
                    output.push_str(&self.padded_line(line));
                }
                if self.vertical_padding {
                    output.push_str(&self.padded_line(""));
                }
                output
            }
            BlockMode::Border => {
                let mut output = String::new();
                output.push_str(&self.top_border_line());
                for line in lines {
                    output.push_str(&self.border_content_line(line));
                }
                output.push_str(&self.bottom_border_line());
                output
            }
        }
    }

    fn top_border_line(&self) -> String {
        let style = self.border_style;
        if self.width < 2 {
            format!("{style}╭{style:#}\n")
        } else {
            let inner = self.width.saturating_sub(2);
            format!("{style}╭{}╮{style:#}\n", "─".repeat(inner))
        }
    }

    fn bottom_border_line(&self) -> String {
        let style = self.border_style;
        if self.width < 2 {
            format!("{style}╰{style:#}\n")
        } else {
            let inner = self.width.saturating_sub(2);
            format!("{style}╰{}╯{style:#}\n", "─".repeat(inner))
        }
    }

    fn border_content_line(&self, content: &str) -> String {
        let content = content.trim_matches('\r');
        let style = self.border_style;
        if self.width < 2 {
            return format!("{style}│{style:#}\n");
        }
        if self.width < 4 {
            let inner = self.width.saturating_sub(2);
            let text = if inner == 0 { "" } else { content };
            let visible = visible_width(text);
            let trailing = inner.saturating_sub(visible);
            return format!(
                "{style}│{style:#}{text}\x1b[0m{}{style}│{style:#}\n",
                " ".repeat(trailing)
            );
        }
        let inner_width = self.width.saturating_sub(4);
        let visible = visible_width(content);
        let trailing = inner_width.saturating_sub(visible);
        format!(
            "{style}│{style:#} {content}\x1b[0m{}{style} │{style:#}\n",
            " ".repeat(trailing)
        )
    }

    fn padded_line(&self, content: &str) -> String {
        let content = content.trim_matches('\r');
        let pad = if self.width >= HORIZONTAL_PADDING * 2 {
            HORIZONTAL_PADDING
        } else {
            0
        };
        let visible = visible_width(content);
        let occupied = pad.saturating_add(visible);
        let trailing = self.width.saturating_sub(occupied);
        let style = self.style;
        let bg_str = style.render().to_string();
        let reset_str = if bg_str.is_empty() {
            String::new()
        } else {
            "\x1b[0m".to_string()
        };
        format!(
            "{style}{}{content}{style}{}{reset_str}\n",
            " ".repeat(pad),
            " ".repeat(trailing)
        )
    }
}

pub fn terminal_width() -> usize {
    crossterm::terminal::size()
        .map(|(columns, _)| usize::from(columns.saturating_sub(1).max(1)))
        .unwrap_or(79)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anstyle::{AnsiColor, Color};

    fn background() -> Style {
        Style::new().bg_color(Some(Color::Ansi(AnsiColor::Black)))
    }

    #[test]
    fn plain_blocks_wrap_and_pad_to_the_requested_width() {
        let rendered = BlockFormat::new(background(), 6)
            .with_vertical_padding()
            .render_plain("abcdefghij");
        let lines: Vec<&str> = rendered.lines().collect();
        assert_eq!(lines.len(), 5);
        assert!(lines.iter().all(|line| visible_width(line) == 6));
        assert!(rendered.contains("abcd"));
        assert!(rendered.contains("efgh"));
        assert!(rendered.contains("ij"));
    }

    #[test]
    fn styled_blocks_wrap_to_full_width_and_preserve_active_color() {
        let rendered = BlockFormat::new(background(), 8).render_styled("\x1b[36mabcdefghijkl\x1b[0m");
        let lines: Vec<&str> = rendered.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines.iter().all(|line| visible_width(line) == 8));
        assert!(lines[0].starts_with("\x1b[40m \x1b[36m"));
        assert!(lines[1].contains("\x1b[36mghijkl"));
    }

    #[test]
    fn styled_content_keeps_its_background_after_an_inner_reset() {
        let rendered = BlockFormat::new(background(), 20).render_line("\x1b[31merror\x1b[0m text");
        assert_eq!(visible_width(&rendered), 20);
        assert!(rendered.contains("\x1b[0m\x1b[40m text"));
        assert!(rendered.ends_with("\x1b[0m"));
    }

    fn assert_lines_padded_and_styled(lines: &[&str], width: usize, bg: &str) {
        for line in lines {
            assert_eq!(visible_width(line), width);
            assert!(line.starts_with(bg));
        }
    }

    #[test]
    fn multiline_styled_blocks_preserve_background_across_resets_and_blank_lines() {
        let content = "\x1b[1m\x1b[31mbold red\x1b[0m\n\n\x1b[32m+ line 2\x1b[0m extra";
        let rendered = BlockFormat::new(background(), 24)
            .with_vertical_padding()
            .render_styled(content);
        let lines: Vec<&str> = rendered.lines().collect();
        assert_eq!(lines.len(), 5);
        assert_lines_padded_and_styled(&lines, 24, "\x1b[40m");
        for line in &lines {
            assert!(line.starts_with("\x1b[40m "));
        }
        assert!(rendered.contains("\x1b[0m\x1b[40m extra"));
    }

    #[test]
    fn border_blocks_render_with_outline_and_fit_requested_width() {
        let border_style = Style::new().fg_color(Some(Color::Ansi(AnsiColor::Green)));
        let rendered = BlockFormat::border(border_style, 20)
            .with_vertical_padding()
            .render_plain("hello border world");
        let lines: Vec<&str> = rendered.lines().collect();
        assert!(lines.len() >= 3);
        assert!(lines.iter().all(|line| visible_width(line) == 20));
        assert!(lines.first().unwrap().contains('╭'));
        assert!(lines.first().unwrap().contains('╮'));
        assert!(lines.last().unwrap().contains('╰'));
        assert!(lines.last().unwrap().contains('╯'));
        assert!(lines[1].contains('│'));
        assert!(lines[1].contains("hello border"));
    }

    #[test]
    fn border_blocks_render_styled_content_and_handle_narrow_widths() {
        let border_style = Style::new().fg_color(Some(Color::Ansi(AnsiColor::Blue)));
        let rendered = BlockFormat::border(border_style, 10).render_styled("\x1b[32mok\x1b[0m");
        let lines: Vec<&str> = rendered.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines.iter().all(|line| visible_width(line) == 10));

        let narrow = BlockFormat::border(border_style, 2).render_plain("x");
        let narrow_lines: Vec<&str> = narrow.lines().collect();
        assert_eq!(narrow_lines.len(), 3);
        assert!(narrow_lines.iter().all(|line| visible_width(line) == 2));
    }

    #[test]
    fn border_content_line_resets_attributes_before_trailing_padding() {
        let border_style = Style::new().fg_color(Some(Color::Ansi(AnsiColor::Blue)));
        let rendered = BlockFormat::border(border_style, 30).render_styled("word \x1b[7mhighlight\x1b[27m");
        let lines: Vec<&str> = rendered.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[1].contains("\x1b[27m\x1b[0m"));
    }

    #[test]
    fn plain_blocks_wrap_sentences_on_word_boundaries() {
        let rendered = BlockFormat::new(background(), 16).render_plain("alpha beta gamma delta");
        let lines: Vec<&str> = rendered.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("alpha beta"));
        assert!(lines[1].contains("gamma delta"));
    }

    #[test]
    fn styled_blocks_wrap_on_word_boundaries_and_preserve_style() {
        let content = "\x1b[32mfirst second third fourth\x1b[0m";
        let rendered = BlockFormat::new(background(), 16).render_styled(content);
        let lines: Vec<&str> = rendered.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("\x1b[32mfirst second"));
        assert!(lines[1].contains("\x1b[32mthird fourth\x1b[0m"));
    }

    #[test]
    fn border_blocks_strip_carriage_returns_to_protect_borders() {
        let border_style = Style::new().fg_color(Some(Color::Ansi(AnsiColor::Green)));
        let rendered = BlockFormat::border(border_style, 30).render_styled("\r00:01 +0: loading\r\n\r00:02 +1: passed");
        let lines: Vec<&str> = rendered.lines().collect();
        assert!(lines.len() >= 4);
        for line in &lines {
            assert!(!line.contains('\r'), "rendered box must not contain carriage return");
            assert_eq!(visible_width(line), 30);
        }
        assert!(lines[1].starts_with("\x1b[32m│\x1b[0m 00:01 +0: loading"));
        assert!(lines[1].ends_with("\x1b[32m │\x1b[0m"));
    }

    #[test]
    fn border_blocks_wrap_long_urls_without_exceeding_card_width() {
        let border_style = Style::new();
        let urls = [
            "https://raw.githubusercontent.com/ornith-ai/Tokenless-Claw-Code/main/CLAW.md",
            "https://raw.githubusercontent.com/ornith-ai/Tokenless-Claw-Code/main/PARITY.md",
            "https://raw.githubusercontent.com/ornith-ai/Tokenless-Claw-Code/main/rust/Cargo.toml",
            "https://api.github.com/repos/ornith-ai/Tokenless-Claw-Code/git/trees/main?recursive=1",
            "https://api.github.com/repos/ornith-ai/Tokenless-Claw-Code/commits",
        ];
        for url in urls {
            let content = format!("\x1b[1mweb_fetch\x1b[0m \x1b[36m{url}\x1b[0m\nfetched (text)");
            let rendered = BlockFormat::border(border_style, 78).render_styled(&content);
            for line in rendered.lines() {
                assert_eq!(visible_width(line), 78, "line '{line}' exceeded width 78 for url {url}");
            }
            let lines: Vec<&str> = rendered.lines().collect();
            if lines.len() > 4 {
                let stripped = strip_ansi(lines[2]);
                assert!(stripped.contains("│ https://"), "expected single space before url, got: {stripped}");
                assert!(!stripped.contains("│  https://"), "unexpected leading double space: {stripped}");
            }
        }
    }
}
