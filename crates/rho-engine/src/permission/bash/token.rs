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
