use super::operator::is_separator_or_redirect;
use super::token::{Token, TokenKind};

struct QuoteBuf<'a> {
    raw: &'a mut String,
    text: &'a mut String,
}

pub(crate) fn read_word(chars: &[char], index: &mut usize) -> (Token, bool, bool) {
    let mut raw = String::new();
    let mut text = String::new();
    let mut suspicious = false;

    while *index < chars.len() {
        let c = chars[*index];
        if is_word_boundary(c) {
            break;
        }
        if c == '\'' {
            let mut buf = QuoteBuf {
                raw: &mut raw,
                text: &mut text,
            };
            if handle_single_quote(chars, index, &mut buf) {
                return (
                    Token {
                        kind: TokenKind::Word,
                        raw,
                        text,
                    },
                    true,
                    true,
                );
            }
            continue;
        }
        if c == '"' {
            let mut buf = QuoteBuf {
                raw: &mut raw,
                text: &mut text,
            };
            let (susp, unterminated) = handle_double_quote(chars, index, &mut buf);
            suspicious = suspicious || susp;
            if unterminated {
                return (
                    Token {
                        kind: TokenKind::Word,
                        raw,
                        text,
                    },
                    true,
                    true,
                );
            }
            continue;
        }
        if c == '\\' && *index + 1 < chars.len() {
            raw.push(c);
            raw.push(chars[*index + 1]);
            text.push(chars[*index + 1]);
            *index += 2;
            continue;
        }
        suspicious = suspicious || check_char_suspicious(chars, *index);
        raw.push(c);
        text.push(c);
        *index += 1;
    }
    (
        Token {
            kind: TokenKind::Word,
            raw,
            text,
        },
        suspicious,
        false,
    )
}

fn is_word_boundary(c: char) -> bool {
    c == ' ' || c == '\t' || c == '\r' || c == '\n' || is_separator_or_redirect(c)
}

fn check_char_suspicious(chars: &[char], idx: usize) -> bool {
    let c = chars[idx];
    if c == '(' || c == ')' || c == '`' {
        return true;
    }
    c == '$' && chars.get(idx + 1).is_some_and(|&next| next == '(' || next == '`')
}

fn handle_single_quote(chars: &[char], index: &mut usize, buf: &mut QuoteBuf<'_>) -> bool {
    buf.raw.push('\'');
    *index += 1;
    let start = *index;
    while *index < chars.len() && chars[*index] != '\'' {
        *index += 1;
    }
    if *index >= chars.len() {
        let remaining: String = chars[start..].iter().collect();
        buf.raw.push_str(&remaining);
        buf.text.push_str(&remaining);
        return true;
    }
    let inside: String = chars[start..*index].iter().collect();
    buf.raw.push_str(&inside);
    buf.raw.push('\'');
    buf.text.push_str(&inside);
    *index += 1;
    false
}

fn handle_double_quote(chars: &[char], index: &mut usize, buf: &mut QuoteBuf<'_>) -> (bool, bool) {
    buf.raw.push('"');
    *index += 1;
    let mut suspicious = false;
    while *index < chars.len() {
        let c = chars[*index];
        if c == '"' {
            buf.raw.push('"');
            *index += 1;
            return (suspicious, false);
        }
        if c == '\\' && *index + 1 < chars.len() {
            buf.raw.push(c);
            buf.raw.push(chars[*index + 1]);
            buf.text.push(chars[*index + 1]);
            *index += 2;
            continue;
        }
        suspicious = suspicious || check_char_suspicious(chars, *index);
        buf.raw.push(c);
        buf.text.push(c);
        *index += 1;
    }
    (true, true)
}
