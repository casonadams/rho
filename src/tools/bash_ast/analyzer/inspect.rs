//! Word and expansion inspection.
//!
//! These methods operate on shell [`Word`](brush_parser::ast::Word) values and
//! raw arithmetic sources, resolving them into literal strings when every
//! piece is statically known. Whenever a piece depends on a parameter
//! expansion, command substitution, or arithmetic expression the analyzer
//! flags the command as [`RiskTier::Mutating`](super::super::types::RiskTier::Mutating).
//!
//! [`super::visit`] walks the AST shape and hands each `Word` off to
//! [`Analyzer::inspect_argument_word`] or [`Analyzer::inspect_command_word`].

use super::super::types::RiskTier;
use super::Analyzer;
use brush_parser::ast::Word;
use brush_parser::word::{self, WordPiece, WordPieceWithSource};

impl Analyzer {
    pub(super) fn inspect_argument_word(&mut self, word: &Word, depth: usize) -> Option<String> {
        self.inspect_word(word, (false, depth))
    }

    pub(super) fn inspect_command_word(&mut self, word: &Word, depth: usize) -> Option<String> {
        self.inspect_word(word, (true, depth))
    }

    pub(super) fn inspect_word(&mut self, word: &Word, context: (bool, usize)) -> Option<String> {
        let (command_position, depth) = context;
        if depth > super::MAX_AST_DEPTH {
            self.flag_depth_overrun();
            return None;
        }
        let options = self.options().clone();
        let Ok(pieces) = word::parse(&word.value, &options) else {
            self.flag(RiskTier::Mutating, "Shell word expansion could not be parsed safely");
            return None;
        };
        self.inspect_pieces(&pieces, (command_position, depth + 1))
    }

    pub(super) fn inspect_pieces(&mut self, pieces: &[WordPieceWithSource], context: (bool, usize)) -> Option<String> {
        let (command_position, depth) = context;
        let mut literal = String::new();
        let mut is_literal = true;
        for piece in pieces {
            match &piece.piece {
                WordPiece::Text(text)
                | WordPiece::SingleQuotedText(text)
                | WordPiece::AnsiCQuotedText(text)
                | WordPiece::EscapeSequence(text) => literal.push_str(text),
                WordPiece::DoubleQuotedSequence(nested) | WordPiece::GettextDoubleQuotedSequence(nested) => {
                    if let Some(text) = self.inspect_pieces(nested, (command_position, depth + 1)) {
                        literal.push_str(&text);
                    } else {
                        is_literal = false;
                    }
                }
                WordPiece::CommandSubstitution(source) | WordPiece::BackquotedCommandSubstitution(source) => {
                    self.visit_substitution(source, depth + 1);
                    is_literal = false;
                }
                WordPiece::ArithmeticExpression(expression) => {
                    self.inspect_arithmetic(&expression.value, depth + 1);
                    is_literal = false;
                    if command_position {
                        self.flag(
                            RiskTier::Mutating,
                            "Dynamic expansion in command position cannot be resolved safely",
                        );
                    }
                }
                WordPiece::ParameterExpansion(_) | WordPiece::TildeExpansion(_) => {
                    is_literal = false;
                    if command_position {
                        self.flag(
                            RiskTier::Mutating,
                            "Dynamic expansion in command position cannot be resolved safely",
                        );
                    }
                }
            }
        }
        is_literal.then_some(literal)
    }

    pub(super) fn inspect_arithmetic(&mut self, source: &str, depth: usize) {
        let options = self.options().clone();
        let Ok(pieces) = word::parse(source, &options) else {
            self.flag(
                RiskTier::Mutating,
                "Shell arithmetic expansion could not be parsed safely",
            );
            return;
        };
        self.inspect_pieces(&pieces, (false, depth + 1));
    }
}
