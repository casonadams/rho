//! A static review of the code a change adds: smells, clean-code violations, and duplication.
//!
//! The core is pure: it reads a unified diff and a `Corpus` of existing files, and runs no checks of
//! the project itself (no tests, linters, or cognitive complexity).

mod diff;
mod dry;
mod functions;
mod lang;
mod smells;
mod source;

pub use dry::Corpus;
pub use lang::{Lang, is_ignored_path};

use source::Source;
use std::collections::HashSet;
use std::fmt::Write as _;

const MAX_FINDINGS: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub path: String,
    pub line: usize,
    /// Last line of a span, for findings that cover several lines.
    pub end_line: Option<usize>,
    pub rule: &'static str,
    pub severity: Severity,
    pub message: String,
}

impl Finding {
    #[must_use]
    pub fn new(path: &str, line: usize, rule: &'static str, severity: Severity, message: &str) -> Self {
        Self {
            path: path.to_string(),
            line,
            end_line: None,
            rule,
            severity,
            message: message.to_string(),
        }
    }

    #[must_use]
    pub fn spanning(mut self, end_line: usize) -> Self {
        self.end_line = Some(end_line);
        self
    }

    fn render(&self) -> String {
        let at = self
            .end_line
            .map_or(self.line.to_string(), |end| format!("{}-{end}", self.line));
        format!("{}:{at} [{}] {}", self.path, self.rule, self.message)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Review {
    pub findings: Vec<Finding>,
    /// Findings dropped because there were more than the review keeps.
    pub truncated: usize,
    pub files_reviewed: usize,
}

fn plural(n: usize, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

/// Reviews the added lines of `diff` (`git diff` output; use large context for whole-file rules).
#[must_use]
pub fn review(diff: &str, corpus: &dyn Corpus) -> Review {
    let sources: Vec<Source> = diff::parse(diff)
        .iter()
        .filter(|f| !lang::is_ignored_path(&f.path))
        .map(Source::from_changed)
        .collect();
    let mut findings: Vec<Finding> = sources.iter().flat_map(smells::check).collect();
    let code: Vec<Source> = sources.iter().filter(|s| s.lang.is_code()).cloned().collect();
    findings.extend(dry::find(&code, corpus));
    findings.sort_by(|a, b| (a.severity, &a.path, a.line, a.rule).cmp(&(b.severity, &b.path, b.line, b.rule)));
    findings.dedup();
    let truncated = findings.len().saturating_sub(MAX_FINDINGS);
    findings.truncate(MAX_FINDINGS);
    Review {
        findings,
        truncated,
        files_reviewed: sources.len(),
    }
}

impl Review {
    #[must_use]
    pub fn counts(&self) -> (usize, usize) {
        let errors = self.findings.iter().filter(|f| f.severity == Severity::Error).count();
        (errors, self.findings.len() - errors)
    }

    fn headline(&self) -> String {
        let (errors, warnings) = self.counts();
        if self.findings.is_empty() {
            return "no findings".to_string();
        }
        format!("{}, {}", plural(errors, "error"), plural(warnings, "warning"))
    }

    /// One line for a subagent result note.
    #[must_use]
    pub fn summary_line(&self) -> String {
        format!("review: {}", self.headline())
    }

    /// The report: a header, up to `max` findings (errors first), and what to do next.
    #[must_use]
    pub fn render(&self, max: usize) -> String {
        if self.findings.is_empty() {
            return "Review: no findings\nThis review is static: project checks (tests, linters, complexity) were NOT run, so run them.".to_string();
        }
        let files: HashSet<&str> = self.findings.iter().map(|f| f.path.as_str()).collect();
        let mut out = format!("Review: {} in {}\n", self.headline(), plural(files.len(), "file"));
        for finding in self.findings.iter().take(max) {
            let _ = writeln!(out, "{}", finding.render());
        }
        let hidden = self.findings.len().saturating_sub(max) + self.truncated;
        if hidden > 0 {
            let _ = writeln!(out, "[{hidden} more findings not shown]");
        }
        out.push_str(
            "Fix errors first. Send the findings to the subagent by continuing it, or fix them after merging. \
             This review is static: project checks (tests, linters, complexity) were NOT run, so run them.",
        );
        out
    }
}

#[cfg(test)]
mod tests;
