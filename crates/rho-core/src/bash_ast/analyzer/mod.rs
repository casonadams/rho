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
//! The walker is split across three files by responsibility:
//!
//! - [`mod`] (this file): the [`Analyzer`] state, [`Analyzer::finish`],
//!   [`Analyzer::flag`], depth-overrun guard, and the classifier-dispatch
//!   wrappers.
//! - [`visit`]: shape-level AST traversal — list, command, compound, simple,
//!   items, redirects, and substitution.
//! - [`inspect`]: word/expansion inspection — argument words, command words,
//!   word pieces, arithmetic.
//!
//! [`MAX_AST_DEPTH`] guards against adversarial input that nests deeply enough
//! to make the analyzer itself slow; on overflow we short-circuit with a
//! `Mutating` flag and stop descending.

mod inspect;
mod visit;

use super::classifier;
use super::types::{RiskTier, SafetyAnalysis};
use brush_parser::ParserOptions;

/// Hard cap on AST depth. Beyond this we treat the input as unsafe and stop
/// descending.
pub(super) const MAX_AST_DEPTH: usize = 32;

const SUBCOMMAND_TOOLS: &[&str] = &[
    "bun",
    "cargo",
    "docker",
    "gh",
    "git",
    "go",
    "helm",
    "kubectl",
    "npm",
    "pnpm",
    "podman",
    "rustup",
    "systemctl",
    "yarn",
];

pub(super) struct Analyzer {
    options: ParserOptions,
    tier: RiskTier,
    reasons: Vec<String>,
    commands: Vec<String>,
    session_patterns: Option<Vec<String>>,
}

impl Analyzer {
    pub(in crate::bash_ast) fn new(options: ParserOptions) -> Self {
        Self {
            options,
            tier: RiskTier::ReadOnly,
            reasons: Vec::new(),
            commands: Vec::new(),
            session_patterns: Some(Vec::new()),
        }
    }

    pub(in crate::bash_ast) fn finish(mut self) -> SafetyAnalysis {
        self.reasons.sort();
        self.reasons.dedup();
        self.commands.dedup();
        if let Some(patterns) = &mut self.session_patterns {
            patterns.sort();
            patterns.dedup();
        }
        SafetyAnalysis {
            tier: self.tier,
            reasons: self.reasons,
            commands: self.commands,
            session_patterns: self.session_patterns,
        }
    }

    pub(super) fn flag(&mut self, tier: RiskTier, reason: impl Into<String>) {
        self.tier = self.tier.max(tier);
        self.reasons.push(reason.into());
    }

    pub(super) fn record_session_pattern(&mut self, command: &str, arguments: &[String]) {
        if arguments
            .iter()
            .any(|argument| argument.is_empty() || argument.contains(char::is_whitespace))
        {
            self.session_patterns = None;
            return;
        }
        let Some(patterns) = &mut self.session_patterns else {
            return;
        };
        let pattern = if SUBCOMMAND_TOOLS.contains(&command)
            || arguments.first().is_some_and(|argument| argument.starts_with('-'))
        {
            arguments
                .first()
                .map_or_else(|| format!("{command} *"), |argument| format!("{command} {argument} *"))
        } else {
            format!("{command} *")
        };
        patterns.push(pattern);
    }

    pub(super) fn disable_session_patterns(&mut self) {
        self.session_patterns = None;
    }

    pub(super) fn flag_depth_overrun(&mut self) {
        self.flag(
            RiskTier::Mutating,
            "Shell AST nesting exceeds the safety analysis limit",
        );
    }

    pub(super) fn options(&self) -> &ParserOptions {
        &self.options
    }

    /// Classify a resolved invocation. The decision tree lives in
    /// [`classifier::classify_invocation`]; this thin wrapper exists so that
    /// the visitor can call `self.classify_invocation(...)` without leaking
    /// `self` into the classifier module.
    pub(super) fn classify_invocation(&mut self, command: &str, arguments: &[String]) {
        classifier::classify_invocation(self, command, arguments);
    }

    pub(super) fn classify_output_redirect(
        &mut self,
        kind: &brush_parser::ast::IoFileRedirectKind,
        target: &brush_parser::ast::IoFileRedirectTarget,
    ) {
        classifier::classify_output_redirect(self, kind, target);
    }

    pub(super) fn classify_output_path(&mut self, raw: &str) {
        classifier::classify_output_path(self, raw);
    }
}
