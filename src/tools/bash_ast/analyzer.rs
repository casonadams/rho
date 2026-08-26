//! AST walker for the brush parser output.
//!
//! The [`Analyzer`] is a single-pass visitor over the parsed program. It
//! accumulates the highest [`RiskTier`](super::types::RiskTier) it sees, a
//! deduped list of reasons, and a deduped list of base command names.
//!
//! All classification logic (which commands and argument patterns are safe)
//! lives in the sibling [`super::classifier`] module — this module is purely
//! about walking the AST and dispatching to the classifier with the resolved
//! command name and arguments.
//!
//! [`MAX_AST_DEPTH`] guards against adversarial input that nests deeply enough
//! to make the analyzer itself slow; on overflow we short-circuit with a
//! `Mutating` flag and stop descending.

use super::classifier;
use super::types::{RiskTier, SafetyAnalysis};
use brush_parser::ast::{
    Command, CommandPrefixOrSuffixItem, CompoundCommand, CompoundList, ExtendedTestExpr, IoFileRedirectKind,
    IoFileRedirectTarget, IoRedirect, RedirectList, SimpleCommand, Word,
};
use brush_parser::word::{self, WordPiece, WordPieceWithSource};
use brush_parser::{Parser, ParserOptions};
use std::io::Cursor;
use std::path::Path;

/// Hard cap on AST depth. Beyond this we treat the input as unsafe and stop
/// descending.
const MAX_AST_DEPTH: usize = 32;

pub(super) struct Analyzer {
    options: ParserOptions,
    tier: RiskTier,
    reasons: Vec<String>,
    commands: Vec<String>,
}

impl Analyzer {
    pub(super) fn new(options: ParserOptions) -> Self {
        Self {
            options,
            tier: RiskTier::ReadOnly,
            reasons: Vec::new(),
            commands: Vec::new(),
        }
    }

    pub(super) fn finish(mut self) -> SafetyAnalysis {
        self.reasons.sort();
        self.reasons.dedup();
        self.commands.dedup();
        SafetyAnalysis {
            tier: self.tier,
            reasons: self.reasons,
            commands: self.commands,
        }
    }

    pub(super) fn flag(&mut self, tier: RiskTier, reason: impl Into<String>) {
        self.tier = self.tier.max(tier);
        self.reasons.push(reason.into());
    }

    fn flag_depth_overrun(&mut self) {
        self.flag(
            RiskTier::Mutating,
            "Shell AST nesting exceeds the safety analysis limit",
        );
    }

    pub(super) fn visit_list(&mut self, list: &CompoundList, depth: usize) {
        if depth > MAX_AST_DEPTH {
            self.flag_depth_overrun();
            return;
        }
        for item in &list.0 {
            for (_, pipeline) in &item.0 {
                for command in &pipeline.seq {
                    self.visit_command(command, depth + 1);
                }
            }
        }
    }

    fn visit_command(&mut self, command: &Command, depth: usize) {
        if depth > MAX_AST_DEPTH {
            self.flag_depth_overrun();
            return;
        }
        match command {
            Command::Simple(simple) => self.visit_simple(simple, depth + 1),
            Command::Compound(compound, redirects) => {
                self.visit_compound(compound, depth + 1);
                if let Some(redirects) = redirects {
                    self.visit_redirects(redirects, depth + 1);
                }
            }
            Command::ExtendedTest(test, redirects) => {
                self.visit_extended_test(&test.expr, depth + 1);
                if let Some(redirects) = redirects {
                    self.visit_redirects(redirects, depth + 1);
                }
            }
            Command::Function(function) => {
                self.flag(
                    RiskTier::Mutating,
                    format!(
                        "Shell function definition is dynamically callable: {}",
                        function.fname.value
                    ),
                );
                self.visit_compound(&function.body.0, depth + 1);
                if let Some(redirects) = &function.body.1 {
                    self.visit_redirects(redirects, depth + 1);
                }
            }
        }
    }

    fn visit_compound(&mut self, command: &CompoundCommand, depth: usize) {
        if depth > MAX_AST_DEPTH {
            self.flag_depth_overrun();
            return;
        }
        match command {
            CompoundCommand::BraceGroup(group) => self.visit_list(&group.list, depth + 1),
            CompoundCommand::Subshell(subshell) => self.visit_list(&subshell.list, depth + 1),
            CompoundCommand::ForClause(clause) => {
                if let Some(values) = &clause.values {
                    for value in values {
                        self.inspect_argument_word(value, depth + 1);
                    }
                }
                self.visit_list(&clause.body.list, depth + 1);
            }
            CompoundCommand::CaseClause(clause) => {
                self.inspect_argument_word(&clause.value, depth + 1);
                for case in &clause.cases {
                    for pattern in &case.patterns {
                        self.inspect_argument_word(pattern, depth + 1);
                    }
                    if let Some(list) = &case.cmd {
                        self.visit_list(list, depth + 1);
                    }
                }
            }
            CompoundCommand::IfClause(clause) => {
                self.visit_list(&clause.condition, depth + 1);
                self.visit_list(&clause.then, depth + 1);
                if let Some(elses) = &clause.elses {
                    for else_clause in elses {
                        if let Some(condition) = &else_clause.condition {
                            self.visit_list(condition, depth + 1);
                        }
                        self.visit_list(&else_clause.body, depth + 1);
                    }
                }
            }
            CompoundCommand::WhileClause(clause) | CompoundCommand::UntilClause(clause) => {
                self.visit_list(&clause.0, depth + 1);
                self.visit_list(&clause.1.list, depth + 1);
            }
            CompoundCommand::Coprocess(coprocess) => {
                self.flag(RiskTier::Mutating, "Coprocess execution requires approval");
                self.visit_command(&coprocess.body, depth + 1);
            }
            CompoundCommand::Arithmetic(command) => {
                self.flag(RiskTier::Mutating, "Shell arithmetic may mutate dynamic variables");
                self.inspect_arithmetic(&command.expr.value, depth + 1);
            }
            CompoundCommand::ArithmeticForClause(clause) => {
                self.flag(RiskTier::Mutating, "Shell arithmetic may mutate dynamic variables");
                for expression in [&clause.initializer, &clause.condition, &clause.updater]
                    .into_iter()
                    .flatten()
                {
                    self.inspect_arithmetic(&expression.value, depth + 1);
                }
                self.visit_list(&clause.body.list, depth + 1);
            }
        }
    }

    fn visit_extended_test(&mut self, expression: &ExtendedTestExpr, depth: usize) {
        match expression {
            ExtendedTestExpr::And(left, right) | ExtendedTestExpr::Or(left, right) => {
                self.visit_extended_test(left, depth + 1);
                self.visit_extended_test(right, depth + 1);
            }
            ExtendedTestExpr::Not(nested) | ExtendedTestExpr::Parenthesized(nested) => {
                self.visit_extended_test(nested, depth + 1);
            }
            ExtendedTestExpr::UnaryTest(_, word) => {
                self.inspect_argument_word(word, depth + 1);
            }
            ExtendedTestExpr::BinaryTest(_, left, right) => {
                self.inspect_argument_word(left, depth + 1);
                self.inspect_argument_word(right, depth + 1);
            }
        }
    }

    fn visit_simple(&mut self, command: &SimpleCommand, depth: usize) {
        if depth > MAX_AST_DEPTH {
            self.flag_depth_overrun();
            return;
        }
        let mut arguments = Vec::new();
        if let Some(prefix) = &command.prefix {
            self.visit_items(&prefix.0, (&mut arguments, depth + 1));
        }

        let Some(command_word) = &command.word_or_name else {
            self.flag(
                RiskTier::Mutating,
                "Shell assignment without a command mutates shell state",
            );
            return;
        };
        let command_name = self.inspect_command_word(command_word, depth + 1);

        if let Some(suffix) = &command.suffix {
            self.visit_items(&suffix.0, (&mut arguments, depth + 1));
        }

        let Some(command_name) = command_name else {
            self.flag(
                RiskTier::Mutating,
                "Dynamic expansion in command position cannot be resolved safely",
            );
            return;
        };
        let base = Path::new(&command_name)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(&command_name)
            .to_ascii_lowercase();
        self.commands.push(base.clone());
        self.classify_invocation(&base, &arguments);
    }

    fn visit_items(&mut self, items: &[CommandPrefixOrSuffixItem], context: (&mut Vec<String>, usize)) {
        let (arguments, depth) = context;
        for item in items {
            match item {
                CommandPrefixOrSuffixItem::Word(word) => {
                    arguments.push(self.inspect_argument_word(word, depth + 1).unwrap_or_default());
                }
                CommandPrefixOrSuffixItem::AssignmentWord(_, word) => {
                    self.inspect_argument_word(word, depth + 1);
                }
                CommandPrefixOrSuffixItem::IoRedirect(redirect) => self.visit_redirect(redirect, depth + 1),
                CommandPrefixOrSuffixItem::ProcessSubstitution(_, subshell) => {
                    self.visit_list(&subshell.list, depth + 1);
                }
            }
        }
    }

    fn inspect_argument_word(&mut self, word: &Word, depth: usize) -> Option<String> {
        self.inspect_word(word, (false, depth))
    }

    fn inspect_command_word(&mut self, word: &Word, depth: usize) -> Option<String> {
        self.inspect_word(word, (true, depth))
    }

    fn inspect_word(&mut self, word: &Word, context: (bool, usize)) -> Option<String> {
        let (command_position, depth) = context;
        if depth > MAX_AST_DEPTH {
            self.flag_depth_overrun();
            return None;
        }
        let Ok(pieces) = word::parse(&word.value, &self.options) else {
            self.flag(RiskTier::Mutating, "Shell word expansion could not be parsed safely");
            return None;
        };
        self.inspect_pieces(&pieces, (command_position, depth + 1))
    }

    fn inspect_pieces(&mut self, pieces: &[WordPieceWithSource], context: (bool, usize)) -> Option<String> {
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

    fn inspect_arithmetic(&mut self, source: &str, depth: usize) {
        let Ok(pieces) = word::parse(source, &self.options) else {
            self.flag(
                RiskTier::Mutating,
                "Shell arithmetic expansion could not be parsed safely",
            );
            return;
        };
        self.inspect_pieces(&pieces, (false, depth + 1));
    }

    fn visit_substitution(&mut self, source: &str, depth: usize) {
        if depth > MAX_AST_DEPTH {
            self.flag_depth_overrun();
            return;
        }
        let mut parser = Parser::new(Cursor::new(source.as_bytes()), &self.options);
        match parser.parse_program() {
            Ok(program) => {
                for command in &program.complete_commands {
                    self.visit_list(command, depth + 1);
                }
            }
            Err(_) => self.flag(RiskTier::Mutating, "Command substitution could not be parsed safely"),
        }
    }

    fn visit_redirects(&mut self, redirects: &RedirectList, depth: usize) {
        for redirect in &redirects.0 {
            self.visit_redirect(redirect, depth + 1);
        }
    }

    fn visit_redirect(&mut self, redirect: &IoRedirect, depth: usize) {
        match redirect {
            IoRedirect::File(_, kind, target) => {
                self.visit_redirect_target(target, depth + 1);
                if !matches!(kind, IoFileRedirectKind::Read | IoFileRedirectKind::DuplicateInput) {
                    self.classify_output_redirect(kind, target);
                }
            }
            IoRedirect::HereDocument(_, document) => {
                if document.requires_expansion {
                    self.inspect_argument_word(&document.doc, depth + 1);
                }
            }
            IoRedirect::HereString(_, word) => {
                self.inspect_argument_word(word, depth + 1);
            }
            IoRedirect::OutputAndError(target, _) => {
                self.inspect_argument_word(target, depth + 1);
                self.classify_output_path(&target.value);
            }
        }
    }

    fn visit_redirect_target(&mut self, target: &IoFileRedirectTarget, depth: usize) {
        match target {
            IoFileRedirectTarget::Filename(word) => {
                self.inspect_argument_word(word, depth + 1);
            }
            IoFileRedirectTarget::Duplicate(word) => {
                if self.inspect_argument_word(word, depth + 1).is_none() {
                    self.flag(
                        RiskTier::Mutating,
                        "Dynamic file descriptor redirection cannot be resolved safely",
                    );
                }
            }
            IoFileRedirectTarget::ProcessSubstitution(_, subshell) => self.visit_list(&subshell.list, depth + 1),
            IoFileRedirectTarget::Fd(_) => {}
        }
    }

    /// Classify a resolved invocation. The decision tree lives in
    /// [`classifier::classify_invocation`]; this thin wrapper exists so that
    /// the visitor can call `self.classify_invocation(...)` without leaking
    /// `self` into the classifier module.
    fn classify_invocation(&mut self, command: &str, arguments: &[String]) {
        classifier::classify_invocation(self, command, arguments);
    }

    pub(super) fn classify_output_redirect(&mut self, kind: &IoFileRedirectKind, target: &IoFileRedirectTarget) {
        classifier::classify_output_redirect(self, kind, target);
    }

    pub(super) fn classify_output_path(&mut self, raw: &str) {
        classifier::classify_output_path(self, raw);
    }
}
