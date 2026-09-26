//! Bash command tokenization and permission analysis.

pub mod analysis;
pub mod danger;
pub mod lexer;

pub use analysis::{BashAnalysis, analyze_bash_command, format_command_lines, has_file_redirection};
pub use danger::is_critical_danger_bash;
pub use lexer::{Token, TokenKind, TokenizerResult, tokenize};
