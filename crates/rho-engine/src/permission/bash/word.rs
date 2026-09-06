use super::operator::is_separator_or_redirect;
use super::token::{Token, TokenKind};

struct QuoteBuf<'a> {
    raw: &'a mut String,
    text: &'a mut String,
}

struct WordReader<'a> {
    chars: &'a [char],
    index: &'a mut usize,
    suspicious: &'a mut bool,
}

fn handle_quote_branch(reader: &mut WordReader<'_>, buf: &mut QuoteBuf<'_>, c: char) -> Option<bool> {
    if c == '\'' {
        return Some(handle_single_quote(reader.chars, reader.index, buf));
    }
    if c == '"' {
        let (susp, unterminated) = handle_double_quote(reader.chars, reader.index, buf);
        *reader.suspicious = *reader.suspicious || susp;
        return Some(unterminated);
    }
    None
}

fn handle_escape_or_char(reader: &mut WordReader<'_>, buf: &mut QuoteBuf<'_>) {
    let c = reader.chars[*reader.index];
    if c == '\\' && *reader.index + 1 < reader.chars.len() {
        buf.raw.push(c);
        buf.raw.push(reader.chars[*reader.index + 1]);
        buf.text.push(reader.chars[*reader.index + 1]);
        *reader.index += 2;
    } else {
        *reader.suspicious = *reader.suspicious || check_char_suspicious(reader.chars, *reader.index);
        buf.raw.push(c);
        buf.text.push(c);
        *reader.index += 1;
    }
}

fn step_read_word(reader: &mut WordReader<'_>, buf: &mut QuoteBuf<'_>, c: char) -> Option<bool> {
    if let Some(unterminated) = handle_quote_branch(reader, buf, c) {
        return Some(unterminated);
    }
    handle_escape_or_char(reader, buf);
    None
}

fn make_word_token(raw: String, text: String) -> Token {
    Token {
        kind: TokenKind::Word,
        raw,
        text,
    }
}

pub(crate) fn read_word(chars: &[char], index: &mut usize) -> (Token, bool, bool) {
    let mut raw = String::new();
    let mut text = String::new();
    let mut suspicious = false;

    while *index < chars.len() {
        if is_word_boundary(chars[*index]) {
            break;
        }
        let mut buf = QuoteBuf {
            raw: &mut raw,
            text: &mut text,
        };
        let mut reader = WordReader {
            chars,
            index,
            suspicious: &mut suspicious,
        };
        let c = chars[*reader.index];
        if let Some(unterminated) = step_read_word(&mut reader, &mut buf, c) {
            if unterminated {
                return (make_word_token(raw, text), true, true);
            }
            continue;
        }
    }
    (make_word_token(raw, text), suspicious, false)
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
