//! Bash command tokenization and permission analysis.

pub mod analysis;
pub mod lexer;
mod normalize;

pub use analysis::{BashAnalysis, analyze_bash_command, format_command_lines, has_file_redirection};
pub use lexer::{Token, TokenKind, TokenizerResult, tokenize};
