//! Shell tokenizer: splits a command string into word, separator, and
//! redirect tokens, flagging suspicious constructs (command substitution,
//! unterminated quotes).

// ---------------------------------------------------------------------------
// Token types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    Word,
    Separator,
    Redirect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub raw: String,
    pub text: String,
}

pub struct TokenizerResult {
    pub tokens: Vec<Token>,
    pub suspicious: bool,
}

// ---------------------------------------------------------------------------
// Operators and separators
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Word reading (quoting and escapes)
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Tokenizer driver
// ---------------------------------------------------------------------------

pub fn tokenize(command: &str) -> TokenizerResult {
    let mut state = TokenizerState::new(command);
    state.run();
    TokenizerResult {
        tokens: state.tokens,
        suspicious: state.suspicious,
    }
}

struct TokenizerState<'a> {
    chars: Vec<char>,
    len: usize,
    index: usize,
    tokens: Vec<Token>,
    suspicious: bool,
    _marker: std::marker::PhantomData<&'a ()>,
}

impl<'a> TokenizerState<'a> {
    fn new(command: &'a str) -> Self {
        let chars: Vec<char> = command.chars().collect();
        let len = chars.len();
        Self {
            chars,
            len,
            index: 0,
            tokens: Vec::new(),
            suspicious: false,
            _marker: std::marker::PhantomData,
        }
    }

    fn step(&mut self) -> bool {
        if self.skip_whitespace() || self.handle_backslash_newline() || self.handle_operator() {
            true
        } else {
            self.handle_word()
        }
    }

    fn run(&mut self) {
        while self.index < self.len && self.step() {}
    }

    fn skip_whitespace(&mut self) -> bool {
        let c = self.chars[self.index];
        if c == ' ' || c == '\t' || c == '\r' {
            self.index += 1;
            true
        } else {
            false
        }
    }

    fn handle_backslash_newline(&mut self) -> bool {
        if self.chars[self.index] == '\\' && self.index + 1 < self.len && self.chars[self.index + 1] == '\n' {
            self.index += 2;
            true
        } else {
            false
        }
    }

    fn handle_operator(&mut self) -> bool {
        if is_operator_start(&self.chars, self.index) {
            let token = read_operator(&self.chars, &mut self.index);
            self.tokens.push(token);
            true
        } else {
            false
        }
    }

    fn handle_word(&mut self) -> bool {
        let word_start = self.index;
        let (token, word_suspicious, unterminated) = read_word(&self.chars, &mut self.index);
        self.suspicious = self.suspicious || word_suspicious;
        self.tokens.push(token);
        if unterminated || self.index == word_start {
            return false;
        }
        self.try_merge_fd();
        true
    }

    fn try_merge_fd(&mut self) {
        if let Some(merged) = read_fd_operator(&self.chars, self.tokens.last(), &mut self.index) {
            let last_idx = self.tokens.len() - 1;
            self.tokens[last_idx] = merged;
        }
    }
}
