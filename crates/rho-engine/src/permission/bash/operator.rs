use super::token::{Token, TokenKind};

pub(crate) fn is_operator_start(chars: &[char], index: usize) -> bool {
    let c = chars[index];
    if c == '\n' || is_separator_or_redirect(c) {
        return true;
    }
    matches!(
        two_chars(chars, index),
        Some("&&") | Some("||") | Some("|&") | Some(";;")
    )
}

pub(crate) fn is_separator_or_redirect(c: char) -> bool {
    c == ';' || c == '|' || c == '&' || c == '>' || c == '<'
}

fn two_chars(chars: &[char], index: usize) -> Option<&'static str> {
    if index + 1 >= chars.len() {
        return None;
    }
    match (chars[index], chars[index + 1]) {
        ('&', '&') => Some("&&"),
        ('|', '|') => Some("||"),
        ('|', '&') => Some("|&"),
        (';', ';') => Some(";;"),
        _ => None,
    }
}

fn make_separator_token(s: &str) -> Token {
    Token {
        kind: TokenKind::Separator,
        raw: s.to_string(),
        text: s.to_string(),
    }
}

pub(crate) fn read_operator(chars: &[char], index: &mut usize) -> Token {
    let c = chars[*index];
    if c == '\n' {
        *index += 1;
        return make_separator_token("\n");
    }
    if let Some(op) = two_chars(chars, *index) {
        *index += 2;
        return make_separator_token(op);
    }
    if c == '>' || c == '<' || (c == '&' && chars.get(*index + 1) == Some(&'>')) {
        return read_redirect_operator(chars, index);
    }
    *index += 1;
    make_separator_token(&c.to_string())
}

fn read_redirect_operator(chars: &[char], index: &mut usize) -> Token {
    let start = *index;
    if chars.get(*index) == Some(&'&') {
        *index += 1;
    }
    while *index < chars.len() && (chars[*index] == '>' || chars[*index] == '<') {
        *index += 1;
    }
    if chars.get(*index) == Some(&'&') {
        *index += 1;
        while *index < chars.len() && chars[*index].is_ascii_digit() {
            *index += 1;
        }
    }
    let raw: String = chars[start..*index].iter().collect();
    Token {
        kind: TokenKind::Redirect,
        text: raw.clone(),
        raw,
    }
}

pub(crate) fn read_fd_operator(chars: &[char], last_token: Option<&Token>, index: &mut usize) -> Option<Token> {
    let last = last_token?;
    if !last.raw.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let next_char = chars.get(*index)?;
    if *next_char != '>' && *next_char != '<' {
        return None;
    }
    let redirect = read_redirect_operator(chars, index);
    let combined = format!("{}{}", last.raw, redirect.raw);
    Some(Token {
        kind: TokenKind::Redirect,
        text: combined.clone(),
        raw: combined,
    })
}
