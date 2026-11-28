//! One reading of terminal output shared by everything that handles styled text: a tokenizer
//! that separates printable characters from escape sequences, and the cursor a terminal advances
//! as it prints them.

use unicode_width::UnicodeWidthChar;

const ESC: char = '\u{1b}';
const BEL: char = '\u{7}';

/// Whether a whole `CSI parameters m` sequence resets every attribute.
pub(crate) fn is_sgr_reset(sequence: &str) -> bool {
    sequence
        .strip_prefix("\u{1b}[")
        .and_then(|body| body.strip_suffix('m'))
        .is_some_and(|parameters| parameters.is_empty() || parameters.parse() == Ok(0u16))
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Token<'a> {
    Char(char),
    /// A whole escape sequence; `sgr` holds the parameters when it is `CSI parameters m`.
    Escape {
        raw: &'a str,
        sgr: Option<&'a str>,
    },
}

pub(crate) fn tokens(text: &str) -> Tokens<'_> {
    Tokens { rest: text }
}

pub(crate) struct Tokens<'a> {
    rest: &'a str,
}

impl<'a> Tokens<'a> {
    fn take(&mut self, len: usize) -> &'a str {
        let (taken, rest) = self.rest.split_at(len);
        self.rest = rest;
        taken
    }

    /// Length through the first character of `after` that satisfies `is_final`, or the whole
    /// input when the sequence never ends.
    fn through(after: &str, offset: usize, is_final: impl Fn(char) -> bool) -> usize {
        after
            .char_indices()
            .find(|&(_, c)| is_final(c))
            .map_or(offset + after.len(), |(at, c)| offset + at + c.len_utf8())
    }

    fn escape(&mut self) -> Token<'a> {
        let after = &self.rest[1..];
        let len = match after.chars().next() {
            Some('[') => Self::through(&after[1..], 2, |c| ('@'..='~').contains(&c)),
            Some(']') => self.operating_system_command(),
            Some(other) => 1 + other.len_utf8(),
            None => 1,
        };
        let raw = self.take(len);
        let sgr = raw.strip_prefix("\u{1b}[").and_then(|body| body.strip_suffix('m'));
        Token::Escape { raw, sgr }
    }

    /// Ends at BEL or at ESC with its following byte (the string terminator).
    fn operating_system_command(&self) -> usize {
        let body = &self.rest[2..];
        match body.char_indices().find(|&(_, c)| c == BEL || c == ESC) {
            Some((at, BEL)) => 2 + at + BEL.len_utf8(),
            Some((at, _)) => {
                let after_esc = 2 + at + ESC.len_utf8();
                after_esc + self.rest[after_esc..].chars().next().map_or(0, char::len_utf8)
            }
            None => self.rest.len(),
        }
    }
}

impl<'a> Iterator for Tokens<'a> {
    type Item = Token<'a>;

    fn next(&mut self) -> Option<Token<'a>> {
        let first = self.rest.chars().next()?;
        if first == ESC {
            return Some(self.escape());
        }
        self.take(first.len_utf8());
        Some(Token::Char(first))
    }
}

/// Where a terminal `width` columns wide puts the next printed character.
#[derive(Debug)]
pub(crate) struct Cursor {
    width: usize,
    column: usize,
    wrap_pending: bool,
}

impl Cursor {
    pub(crate) fn new(width: usize) -> Self {
        Self {
            width: width.max(1),
            column: 0,
            wrap_pending: false,
        }
    }

    /// Column of the next character, which is 0 while a full row waits to wrap.
    pub(crate) fn column(&self) -> usize {
        if self.wrap_pending { 0 } else { self.column }
    }

    /// Whether the last row is exactly full and the next character starts a new one.
    pub(crate) fn wrap_pending(&self) -> bool {
        self.wrap_pending
    }

    pub(crate) fn is_at_line_start(&self) -> bool {
        self.column == 0 && !self.wrap_pending
    }

    pub(crate) fn return_to_line_start(&mut self) {
        self.column = 0;
        self.wrap_pending = false;
    }

    /// Whether printing `character` wraps onto a new row first; zero-width characters never do.
    pub(crate) fn wraps_before(&self, character: char) -> bool {
        let width = character.width().unwrap_or(0);
        width > 0 && (self.wrap_pending || (self.column > 0 && self.column + width > self.width))
    }

    /// Moves past `character`, wrapping first when `wraps_before` says so.
    pub(crate) fn advance(&mut self, character: char) {
        let width = character.width().unwrap_or(0);
        if width == 0 {
            return;
        }
        if self.wraps_before(character) {
            self.column = 0;
        }
        self.column += width;
        self.wrap_pending = self.column >= self.width;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_case::test_case;

    fn escapes(text: &str) -> Vec<(&str, Option<&str>)> {
        tokens(text)
            .filter_map(|token| match token {
                Token::Escape { raw, sgr } => Some((raw, sgr)),
                Token::Char(_) => None,
            })
            .collect()
    }

    #[test]
    fn sequences_are_single_tokens_and_text_is_per_character() {
        let all: Vec<_> = tokens("a\u{1b}[1;31mb\u{1b}]0;t\u{7}c\u{1b}M").collect();
        assert_eq!(
            all,
            [
                Token::Char('a'),
                Token::Escape {
                    raw: "\u{1b}[1;31m",
                    sgr: Some("1;31")
                },
                Token::Char('b'),
                Token::Escape {
                    raw: "\u{1b}]0;t\u{7}",
                    sgr: None
                },
                Token::Char('c'),
                Token::Escape {
                    raw: "\u{1b}M",
                    sgr: None
                },
            ]
        );
    }

    #[test]
    fn only_sgr_sequences_report_style_parameters() {
        assert_eq!(
            escapes("\u{1b}[m\u{1b}[2K\u{1b}[?2026h\u{1b}[0m"),
            [
                ("\u{1b}[m", Some("")),
                ("\u{1b}[2K", None),
                ("\u{1b}[?2026h", None),
                ("\u{1b}[0m", Some("0")),
            ]
        );
    }

    #[test]
    fn a_string_terminator_ends_an_operating_system_command_whole() {
        let all: Vec<_> = tokens("\u{1b}]8;;http://x\u{1b}\\link").collect();
        assert_eq!(
            all[0],
            Token::Escape {
                raw: "\u{1b}]8;;http://x\u{1b}\\",
                sgr: None
            }
        );
        assert_eq!(all[1], Token::Char('l'));
    }

    #[test]
    fn unfinished_sequences_take_the_rest_of_the_input() {
        assert_eq!(escapes("\u{1b}[12"), [("\u{1b}[12", None)]);
        assert_eq!(escapes("\u{1b}]0;title"), [("\u{1b}]0;title", None)]);
        assert_eq!(escapes("x\u{1b}"), [("\u{1b}", None)]);
    }

    #[test_case("\u{1b}[m", true; "no parameters")]
    #[test_case("\u{1b}[0m", true; "zero parameter")]
    #[test_case("\u{1b}[00m", true; "padded zero parameter")]
    #[test_case("\u{1b}[1m", false; "bold")]
    #[test_case("\u{1b}[0;1m", false; "reset then bold")]
    #[test_case("\u{1b}[0K", false; "erase line")]
    #[test_case("0m", false; "missing escape")]
    #[test_case("", false; "empty")]
    fn only_sequences_with_no_or_zero_parameters_reset(sequence: &str, resets: bool) {
        assert_eq!(is_sgr_reset(sequence), resets);
    }

    #[test]
    fn a_full_row_waits_to_wrap_until_the_next_character() {
        let mut cursor = Cursor::new(4);
        "abcd".chars().for_each(|c| cursor.advance(c));
        assert_eq!((cursor.column(), cursor.wrap_pending()), (0, true));
        assert!(cursor.wraps_before('e'));
        assert!(!cursor.wraps_before('\u{301}'));
        cursor.advance('e');
        assert_eq!((cursor.column(), cursor.wrap_pending()), (1, false));
    }

    #[test]
    fn a_wide_character_that_does_not_fit_moves_to_the_next_row() {
        let mut cursor = Cursor::new(4);
        "abc".chars().for_each(|c| cursor.advance(c));
        assert!(cursor.wraps_before('\u{754c}'));
        cursor.advance('\u{754c}');
        assert_eq!(cursor.column(), 2);
    }

    #[test]
    fn a_zero_width_character_keeps_a_pending_wrap() {
        let mut cursor = Cursor::new(2);
        "ab".chars().for_each(|c| cursor.advance(c));
        cursor.advance('\u{301}');
        assert!(cursor.wrap_pending());
    }
}
