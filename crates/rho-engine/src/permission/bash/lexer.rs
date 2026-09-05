use super::operator::{is_operator_start, read_fd_operator, read_operator};
use super::token::{Token, TokenizerResult};
use super::word::read_word;

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

    fn run(&mut self) {
        while self.index < self.len {
            if self.skip_whitespace() {
                continue;
            }
            if self.handle_backslash_newline() {
                continue;
            }
            if self.handle_operator() {
                continue;
            }
            if !self.handle_word() {
                break;
            }
        }
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
