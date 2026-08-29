//! Bash command safety analysis.
//!
//! This module is organised by responsibility:
//!
//! - [`types`]: the [`RiskTier`] enum and [`SafetyAnalysis`] struct — the data
//!   shapes returned to callers.
//! - [`analyzer`]: the [`Analyzer`](analyzer::Analyzer) struct plus all `visit_*`
//!   methods that walk a parsed brush AST and accumulate flags.
//! - [`classifier`]: the command-classification rules ([`classifier::classify_invocation`]),
//!   the read-only allow-list, and the argument heuristics ([`classifier::is_high_risk_command`],
//!   [`classifier::is_read_only_git`], [`classifier::is_read_only_cargo`], etc.).
//!
//! [`analyze_command_safety`] is the public entry point — it parses the input,
//!   drives the analyzer, and returns a [`types::SafetyAnalysis`].
//!
//! Tests live in `tools/bash_ast/tests.rs` and are gated by `#[cfg(test)]`.

mod analyzer;
mod classifier;
mod types;

pub use types::{RiskTier, SafetyAnalysis};

use analyzer::Analyzer;
use brush_parser::{Parser, ParserOptions};
use std::io::Cursor;

/// Parse a shell command and classify it by safety tier.
///
/// On parse failure, empty input, or any unresolvable dynamic expansion the
/// analysis returns [`RiskTier::Mutating`] (never [`RiskTier::ReadOnly`]) so the
/// caller must request user approval rather than skipping it.
pub fn analyze_command_safety(command: &str) -> SafetyAnalysis {
    if command.trim().is_empty() {
        return SafetyAnalysis {
            tier: RiskTier::Mutating,
            reasons: vec!["Empty shell input cannot be proven read-only".to_string()],
            commands: Vec::new(),
            session_patterns: None,
        };
    }

    let options = ParserOptions::default();
    let mut parser = Parser::new(Cursor::new(command.as_bytes()), &options);
    let Ok(program) = parser.parse_program() else {
        return SafetyAnalysis {
            tier: RiskTier::Mutating,
            reasons: vec!["Shell AST parse error; approval is required".to_string()],
            commands: Vec::new(),
            session_patterns: None,
        };
    };

    let mut analyzer = Analyzer::new(options);
    for complete_command in &program.complete_commands {
        analyzer.visit_list(complete_command, 0);
    }
    analyzer.finish()
}

#[cfg(test)]
mod tests;
