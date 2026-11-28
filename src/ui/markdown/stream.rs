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
    italic: Option<char>,
    bold: Option<char>,
    strike: bool,
    in_code: bool,
    prev: Option<char>,
    pending: Option<PendingRun>,
    link: Option<LinkHold>,
    scratch: Vec<char>,
}

#[derive(Clone, Copy)]
struct PendingRun {
    marker: char,
    len: usize,
    prev: Option<char>,
}

struct LinkHold {
    raw: String,
    stage: LinkStage,
}

#[derive(Clone, Copy)]
enum LinkStage {
    Text(usize),
    AfterText,
    Url(usize),
    Angle,
}

enum LinkStep {
    Hold,
    Done,
    NotALink,
}

#[derive(Clone, Copy, PartialEq)]
enum Emphasis {
    Italic,
    Bold,
    BoldItalic,
    Strike,
}

fn is_markdown_punctuation(c: char) -> bool {
    c.is_ascii_punctuation() || (!c.is_alphanumeric() && !c.is_whitespace())
}

fn emphasis_for(marker: char, len: usize) -> Option<Emphasis> {
    match (marker, len) {
        ('*' | '_', 1) => Some(Emphasis::Italic),
        ('*' | '_', 2) => Some(Emphasis::Bold),
        ('*' | '_', 3) => Some(Emphasis::BoldItalic),
        ('~', 2) => Some(Emphasis::Strike),
        _ => None,
    }
}

/// CommonMark flanking rules; `_` may not open or close inside a word.
fn run_can_open_close(marker: char, prev: Option<char>, next: Option<char>) -> (bool, bool) {
    let space = |c: Option<char>| c.is_none_or(char::is_whitespace);
    let punct = |c: Option<char>| c.is_some_and(is_markdown_punctuation);
    let left = !space(next) && (!punct(next) || space(prev) || punct(prev));
    let right = !space(prev) && (!punct(prev) || space(next) || punct(next));
    if marker == '_' {
        (left && (!right || punct(prev)), right && (!left || punct(next)))
    } else {
        (left, right)
    }
}

impl LinkHold {
    fn push(&mut self, c: char) -> LinkStep {
        self.raw.push(c);
        let (stage, step) = match (self.stage, c) {
            (LinkStage::Text(depth), '[') => (LinkStage::Text(depth + 1), LinkStep::Hold),
            (LinkStage::Text(1), ']') => (LinkStage::AfterText, LinkStep::Hold),
            (LinkStage::Text(depth), ']') => (LinkStage::Text(depth - 1), LinkStep::Hold),
            (LinkStage::AfterText, '(') => (LinkStage::Url(1), LinkStep::Hold),
            (LinkStage::AfterText, _) => (LinkStage::AfterText, LinkStep::NotALink),
            (LinkStage::Url(depth), '(') => (LinkStage::Url(depth + 1), LinkStep::Hold),
            (LinkStage::Url(1), ')') => (LinkStage::Url(0), LinkStep::Done),
            (LinkStage::Url(depth), ')') => (LinkStage::Url(depth - 1), LinkStep::Hold),
            (LinkStage::Angle, '>') => (LinkStage::Angle, LinkStep::Done),
            (LinkStage::Angle, c) if c.is_whitespace() || c == '<' => (LinkStage::Angle, LinkStep::NotALink),
            (stage, _) => (stage, LinkStep::Hold),
        };
        self.stage = stage;
        step
    }
}

impl InlineStreamTracker {
    pub fn reset_line(&mut self, theme: &Theme) -> String {
        let mut out = String::new();
        if let Some(link) = self.link.take() {
            self.replay_unlinked(&link.raw, &mut out, theme);
        }
        if let Some(run) = self.pending.take() {
            self.resolve_run(run, None, &mut out);
        }
        self.prev = None;
        if self.bold.take().is_some() {
            out.push_str(&anstyle::Style::new().bold().render_reset().to_string());
        }
        if self.italic.take().is_some() {
            out.push_str(&anstyle::Style::new().italic().render_reset().to_string());
        }
        if std::mem::take(&mut self.strike) {
            out.push_str(&anstyle::Style::new().strikethrough().render_reset().to_string());
        }
        if std::mem::take(&mut self.in_code) {
            out.push_str(&anstyle::Style::new().render_reset().to_string());
        }
        out
    }

    fn is_active(&self, emphasis: Emphasis) -> bool {
        match emphasis {
            Emphasis::Italic => self.italic.is_some(),
            Emphasis::Bold => self.bold.is_some(),
            Emphasis::BoldItalic => self.italic.is_some() && self.bold.is_some(),
            Emphasis::Strike => self.strike,
        }
    }

    fn opened_by(&self, emphasis: Emphasis, marker: char) -> bool {
        match emphasis {
            Emphasis::Italic => self.italic == Some(marker),
            Emphasis::Bold => self.bold == Some(marker),
            Emphasis::BoldItalic => self.italic == Some(marker) && self.bold == Some(marker),
            Emphasis::Strike => self.strike,
        }
    }

    fn set_emphasis(&mut self, emphasis: Emphasis, marker: Option<char>, out: &mut String) {
        let on = marker.is_some();
        let style = match emphasis {
            Emphasis::Italic => anstyle::Style::new().italic(),
            Emphasis::Bold => anstyle::Style::new().bold(),
            Emphasis::BoldItalic => anstyle::Style::new().bold().italic(),
            Emphasis::Strike => anstyle::Style::new().strikethrough(),
        };
        match emphasis {
            Emphasis::Italic => self.italic = marker,
            Emphasis::Bold => self.bold = marker,
            Emphasis::BoldItalic => (self.italic, self.bold) = (marker, marker),
            Emphasis::Strike => self.strike = on,
        }
        let code = if on {
            style.render().to_string()
        } else {
            style.render_reset().to_string()
        };
        out.push_str(&code);
    }

    fn resolve_run(&mut self, run: PendingRun, next: Option<char>, out: &mut String) {
        let (can_open, can_close) = run_can_open_close(run.marker, run.prev, next);
        match emphasis_for(run.marker, run.len) {
            Some(emphasis) if self.is_active(emphasis) && can_close && self.opened_by(emphasis, run.marker) => {
                self.set_emphasis(emphasis, None, out);
            }
            Some(emphasis) if !self.is_active(emphasis) && can_open => {
                self.set_emphasis(emphasis, Some(run.marker), out);
            }
            _ => out.extend(std::iter::repeat_n(run.marker, run.len)),
        }
        self.prev = Some(run.marker);
    }

    fn toggle_code(&mut self, out: &mut String, theme: &Theme) {
        self.in_code = !self.in_code;
        let code = if self.in_code {
            theme.code_inline.render().to_string()
        } else {
            theme.code_inline.render_reset().to_string()
        };
        out.push_str(&code);
    }

    fn handle_run(&mut self, chars: &[char], i: usize, out: &mut String) -> usize {
        let marker = chars[i];
        let len = chars[i..].iter().take_while(|&&c| c == marker).count();
        let run = PendingRun {
            marker,
            len,
            prev: self.prev,
        };
        match chars.get(i + len) {
            Some(&next) => self.resolve_run(run, Some(next), out),
            None => self.pending = Some(run),
        }
        len
    }

    fn continue_pending(&mut self, mut run: PendingRun, chars: &[char], out: &mut String) -> usize {
        let extra = chars.iter().take_while(|&&c| c == run.marker).count();
        run.len += extra;
        match chars.get(extra) {
            Some(&next) => self.resolve_run(run, Some(next), out),
            None => self.pending = Some(run),
        }
        extra
    }

    /// Returns `false` when `c` proved the held text is not a link and must be dispatched again.
    fn hold_link_char(&mut self, mut link: LinkHold, c: char, out: &mut String, theme: &Theme) -> bool {
        match link.push(c) {
            LinkStep::Hold => {
                self.link = Some(link);
                true
            }
            LinkStep::Done => {
                out.push_str(&super::elements::render_inline_elements(&link.raw, theme));
                self.prev = Some(')');
                true
            }
            LinkStep::NotALink => {
                link.raw.pop();
                self.replay_unlinked(&link.raw, out, theme);
                false
            }
        }
    }

    fn replay_unlinked(&mut self, raw: &str, out: &mut String, theme: &Theme) {
        let mut rest = raw.chars();
        if let Some(opener) = rest.next() {
            out.push(opener);
            self.prev = Some(opener);
        }
        let chars: Vec<char> = rest.collect();
        self.process_chars(&chars, out, theme);
    }

    fn process_token_char(&mut self, chars: &[char], i: usize, out: &mut String, theme: &Theme) -> usize {
        let c = chars[i];
        if let Some(link) = self.link.take() {
            if self.hold_link_char(link, c, out, theme) {
                return 1;
            }
            return self.process_token_char(chars, i, out, theme);
        }
        if c == '`' {
            self.toggle_code(out, theme);
        } else if self.in_code {
            out.push(c);
        } else if matches!(c, '*' | '_' | '~') {
            return self.handle_run(chars, i, out);
        } else if c == '[' || c == '<' {
            let stage = if c == '[' { LinkStage::Text(1) } else { LinkStage::Angle };
            self.link = Some(LinkHold {
                raw: String::from(c),
                stage,
            });
        } else {
            out.push(c);
        }
        self.prev = Some(c);
        1
    }

    fn process_chars(&mut self, chars: &[char], out: &mut String, theme: &Theme) {
        let mut i = 0;
        while i < chars.len() {
            i += self.process_token_char(chars, i, out, theme);
        }
    }

    pub fn render_inline_token(&mut self, token: &str, theme: &Theme) -> String {
        let mut out = String::new();
        let mut chars = std::mem::take(&mut self.scratch);
        chars.clear();
        chars.extend(token.chars());
        let mut start = 0;
        if let Some(run) = self.pending.take_if(|_| !chars.is_empty()) {
            start = self.continue_pending(run, &chars, &mut out);
        }
        self.process_chars(&chars[start..], &mut out, theme);
        self.scratch = chars;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_case::test_case;

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

    #[test_case("hello\nworld", "hello\nworld"; "two words")]
    #[test_case("ab  \ncd", "ab\ncd"; "trailing spaces")]
    #[test_case("ab\n", "ab\n"; "final newline")]
    #[test_case("\n\n", "\n\n"; "only newlines")]
    fn handle_newline_commits_the_word_and_drops_trailing_spaces(input: &str, expected: &str) {
        let mut wrapper = StreamWordWrapper::new(20);
        let mut output = wrapper.process_chunk(input);
        output.push_str(&wrapper.flush());
        assert_eq!(output, expected);
    }

    fn render_split(text: &str, split: usize) -> String {
        let theme = Theme::default();
        let mut tracker = InlineStreamTracker::default();
        let (head, tail) = text.split_at(split);
        let mut out = tracker.render_inline_token(head, &theme);
        out.push_str(&tracker.render_inline_token(tail, &theme));
        out.push_str(&tracker.reset_line(&theme));
        out
    }

    #[test_case("an *italic* word", "an italic word"; "italic")]
    #[test_case("ends *italic*", "ends italic"; "trailing italic")]
    #[test_case("a **bold** word", "a bold word"; "bold")]
    #[test_case("math 2 * 3 and a* star", "math 2 * 3 and a* star"; "literal stars")]
    #[test_case("*lead* then *x* end", "lead then x end"; "leading italic")]
    #[test_case("mixed **bold** and *it* and `co*de`", "mixed bold and it and co*de"; "mixed markers")]
    fn inline_markers_render_the_same_at_every_chunk_boundary(text: &str, plain: &str) {
        let whole = render_split(text, 0);
        assert_eq!(crate::ui::block::strip_ansi(&whole), plain);
        for split in 1..text.len() {
            assert_eq!(render_split(text, split), whole, "split at {split}");
        }
    }

    #[test_case("\x1b[1mab", "cdefghij", "cdefg\nhij"; "styled partial word")]
    #[test_case("ab ", "cd", "cd"; "trailing space")]
    #[test_case("abc", "de", "de"; "partial word")]
    fn flush_resets_wrap_state_before_the_next_segment(first: &str, second: &str, expected: &str) {
        let mut wrapper = StreamWordWrapper::new(5);
        wrapper.process_chunk(first);
        wrapper.flush();
        let mut output = wrapper.process_chunk(second);
        output.push_str(&wrapper.flush());
        assert_eq!(output, expected);
    }
}
