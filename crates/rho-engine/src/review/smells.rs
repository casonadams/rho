//! Static smell rules over the added lines of a file.

use super::functions;
use super::lang::{CODE, Lang};
use super::source::{Line, Source, is_char_quote};
use super::{Finding, Severity};
use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;

const MAX_LINE_CHARS: usize = 140;
const MAX_FUNCTION_LINES: usize = 60;
const MAX_PARAMS: usize = 6;
const MAX_NEW_FILE_LINES: usize = 400;
const MAX_NESTING: usize = 5;
const MIN_LITERAL_CHARS: usize = 12;
const MIN_LITERAL_REPEATS: usize = 3;

struct LineRule {
    id: &'static str,
    severity: Severity,
    /// Empty means every file, including non-code files.
    langs: &'static [Lang],
    in_tests: bool,
    in_comments: bool,
    matches: fn(&Line) -> bool,
    message: &'static str,
}

const RUST: &[Lang] = &[Lang::Rust];
const JS: &[Lang] = &[Lang::Js];
const PYTHON: &[Lang] = &[Lang::Python];
const GO: &[Lang] = &[Lang::Go];
const SLASH_COMMENT_LANGS: &[Lang] = &[Lang::Rust, Lang::Js, Lang::Go, Lang::Java, Lang::C];

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).expect("valid smell pattern")
}

static SECRET: LazyLock<Regex> = LazyLock::new(|| {
    re(concat!(
        r#"(?i)\b(?:password|passwd|secret|api[_-]?key|access[_-]?token|auth[_-]?token)\b["']?\s*[:=]\s*["'][^"'\s${}%]{8,}["']"#,
        r"|\bsk-[A-Za-z0-9_-]{20,}|\bghp_[A-Za-z0-9]{20,}|\bAKIA[A-Z0-9]{16}",
        r"|-----BEGIN [A-Z ]*PRIVATE KEY-----|\bbearer\s+[A-Za-z0-9._~+/-]{16,}"
    ))
});
static MARKER_WORD: LazyLock<Regex> = LazyLock::new(|| re(r"\b(?:TODO|FIXME|XXX|HACK)\b"));

fn is_commented_out_code(line: &Line) -> bool {
    let raw = line.raw.as_str();
    if !line.comment || raw.starts_with("///") || raw.starts_with("//!") || raw.starts_with("/*") {
        return false;
    }
    let content = raw.trim_start_matches('/').trim();
    let prose = content.chars().next().is_some_and(char::is_uppercase)
        && content
            .split_whitespace()
            .next()
            .is_some_and(|w| w.chars().all(char::is_alphabetic));
    content.ends_with([';', '{', '}']) && content.contains(['(', '=']) && !prose
}

fn is_long_line(line: &Line) -> bool {
    line.code.chars().count() > MAX_LINE_CHARS && line.raw.contains(' ') && !line.raw.contains("http")
}

const RULES: &[LineRule] = &[
    LineRule {
        id: "secret",
        severity: Severity::Error,
        langs: &[],
        in_tests: false,
        in_comments: true,
        matches: |l| SECRET.is_match(&l.raw),
        message: "looks like a credential; remove it and rotate it if it was real",
    },
    LineRule {
        id: "debug-leftover",
        severity: Severity::Error,
        langs: RUST,
        in_tests: true,
        in_comments: false,
        matches: |l| l.code.contains("dbg!("),
        message: "`dbg!` left in the code",
    },
    LineRule {
        id: "debug-leftover",
        severity: Severity::Error,
        langs: JS,
        in_tests: true,
        in_comments: false,
        matches: |l| l.code.trim_end_matches(';') == "debugger",
        message: "`debugger` statement left in the code",
    },
    LineRule {
        id: "debug-leftover",
        severity: Severity::Error,
        langs: PYTHON,
        in_tests: true,
        in_comments: false,
        matches: |l| l.code.contains("breakpoint()") || l.code.contains("pdb.set_trace"),
        message: "debugger call left in the code",
    },
    LineRule {
        id: "placeholder",
        severity: Severity::Error,
        langs: RUST,
        in_tests: false,
        in_comments: false,
        matches: |l| l.code.contains("todo!(") || l.code.contains("unimplemented!("),
        message: "placeholder macro: the code path is not implemented",
    },
    LineRule {
        id: "placeholder",
        severity: Severity::Error,
        langs: PYTHON,
        in_tests: false,
        in_comments: false,
        matches: |l| l.code.contains("NotImplementedError") || (l.code.starts_with("pass") && l.code.contains("TODO")),
        message: "placeholder: the code path is not implemented",
    },
    LineRule {
        id: "lint-suppression",
        severity: Severity::Warning,
        langs: RUST,
        in_tests: true,
        in_comments: false,
        matches: |l| {
            ["#[allow(", "#![allow(", "#[expect(", "#![expect("]
                .iter()
                .any(|p| l.code.starts_with(p))
        },
        message: "lint suppression: fix the cause instead of silencing the lint",
    },
    LineRule {
        id: "lint-suppression",
        severity: Severity::Warning,
        langs: JS,
        in_tests: true,
        in_comments: true,
        matches: |l| {
            ["eslint-disable", "@ts-ignore", "@ts-expect-error", "biome-ignore"]
                .iter()
                .any(|p| l.code.contains(p))
        },
        message: "lint or type check suppression: fix the cause instead of silencing it",
    },
    LineRule {
        id: "lint-suppression",
        severity: Severity::Warning,
        langs: PYTHON,
        in_tests: true,
        in_comments: true,
        matches: |l| {
            ["# noqa", "# type: ignore", "# pylint: disable"]
                .iter()
                .any(|p| l.code.contains(p))
        },
        message: "lint or type check suppression: fix the cause instead of silencing it",
    },
    LineRule {
        id: "lint-suppression",
        severity: Severity::Warning,
        langs: GO,
        in_tests: true,
        in_comments: true,
        matches: |l| l.code.contains("//nolint"),
        message: "lint suppression: fix the cause instead of silencing the lint",
    },
    LineRule {
        id: "todo-marker",
        severity: Severity::Warning,
        langs: CODE,
        in_tests: true,
        in_comments: true,
        matches: |l| MARKER_WORD.is_match(&l.code),
        message: "unfinished-work marker (TODO/FIXME/XXX/HACK)",
    },
    LineRule {
        id: "unwrap",
        severity: Severity::Warning,
        langs: RUST,
        in_tests: false,
        in_comments: false,
        matches: |l| (l.code.contains(".unwrap()") || l.code.contains(".expect(")) && !l.code.contains("Regex::new("),
        message: "`unwrap`/`expect` can panic in non-test code; propagate the error",
    },
    LineRule {
        id: "console-log",
        severity: Severity::Warning,
        langs: JS,
        in_tests: false,
        in_comments: false,
        matches: |l| l.code.contains("console.log("),
        message: "`console.log` in non-test code",
    },
    LineRule {
        id: "commented-out-code",
        severity: Severity::Warning,
        langs: SLASH_COMMENT_LANGS,
        in_tests: true,
        in_comments: true,
        matches: is_commented_out_code,
        message: "commented-out code; delete it (version control keeps it)",
    },
    LineRule {
        id: "long-line",
        severity: Severity::Warning,
        langs: CODE,
        in_tests: true,
        in_comments: true,
        matches: is_long_line,
        message: "line is longer than 140 characters",
    },
];

fn applies(rule: &LineRule, lang: Lang) -> bool {
    rule.langs.is_empty() || rule.langs.contains(&lang)
}

fn line_findings(source: &Source) -> Vec<Finding> {
    let mut out = Vec::new();
    for line in source.lines.iter().filter(|l| l.added) {
        for rule in RULES.iter().filter(|r| applies(r, source.lang)) {
            let visible = (rule.in_tests || !line.test) && (rule.in_comments || !line.comment);
            if visible && (rule.matches)(line) {
                out.push(Finding::new(
                    &source.path,
                    line.no,
                    rule.id,
                    rule.severity,
                    rule.message,
                ));
            }
        }
    }
    out
}

fn conflict_markers(source: &Source) -> Vec<Finding> {
    let is_marker = |l: &Line| l.raw.starts_with("<<<<<<<") || l.raw.starts_with(">>>>>>>");
    let found = source.lines.iter().any(|l| l.added && is_marker(l));
    source
        .lines
        .iter()
        .filter(|l| l.added && found && (is_marker(l) || l.raw == "======="))
        .map(|l| {
            Finding::new(
                &source.path,
                l.no,
                "conflict-marker",
                Severity::Error,
                "git conflict marker",
            )
        })
        .collect()
}

fn new_file_size(source: &Source) -> Vec<Finding> {
    let big = source.is_new && source.lang.is_code() && source.lines.len() > MAX_NEW_FILE_LINES;
    if !big || source.lines.first().is_some_and(|l| l.test) {
        return Vec::new();
    }
    let message = format!(
        "new file has {} lines (limit {MAX_NEW_FILE_LINES}); split it by responsibility",
        source.lines.len()
    );
    vec![Finding::new(&source.path, 1, "large-file", Severity::Warning, &message)]
}

/// Contents of the string literals in `raw` that are at least `MIN_LITERAL_CHARS` long.
fn string_literals(raw: &str, single_quotes: bool) -> Vec<String> {
    let chars: Vec<char> = raw.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let quote = chars[i];
        if is_char_quote(&chars, i) {
            i += 3;
        } else if quote == '"' || (single_quotes && quote == '\'') {
            let end = (i + 1..chars.len()).find(|&j| {
                chars[j] == quote && chars[i + 1..j].iter().rev().take_while(|&&c| c == '\\').count() % 2 == 0
            });
            let text: String = chars[i + 1..end.unwrap_or(chars.len())].iter().collect();
            if end.is_some() && text.chars().count() >= MIN_LITERAL_CHARS {
                out.push(text);
            }
            i = end.map_or(chars.len(), |j| j + 1);
        } else {
            i += 1;
        }
    }
    out
}

fn repeated_literals(source: &Source) -> Vec<Finding> {
    if !source.lang.is_code() {
        return Vec::new();
    }
    let single = matches!(source.lang, Lang::Js | Lang::Python);
    let mut seen: HashMap<String, (usize, usize)> = HashMap::new();
    let skipped = |l: &Line| l.comment || l.test || ["use ", "import ", "#["].iter().any(|p| l.code.starts_with(p));
    for line in source.lines.iter().filter(|l| l.added && !skipped(l)) {
        for text in string_literals(&line.raw, single) {
            seen.entry(text).or_insert((line.no, 0)).1 += 1;
        }
    }
    let mut repeated: Vec<_> = seen
        .into_iter()
        .filter(|(_, (_, n))| *n >= MIN_LITERAL_REPEATS)
        .collect();
    repeated.sort_by_key(|(_, (line, _))| *line);
    repeated
        .into_iter()
        .map(|(text, (line, n))| {
            let shown: String = text.chars().take(40).collect();
            let message = format!("string literal \"{shown}\" appears {n} times; extract a constant");
            Finding::new(&source.path, line, "repeated-literal", Severity::Warning, &message)
        })
        .collect()
}

fn bool_params(params: &[String]) -> usize {
    let is_bool = |p: &&String| {
        p.ends_with(": bool")
            || p.contains(": bool ")
            || p.contains(": bool=")
            || p.contains(": boolean")
            || p.starts_with("boolean ")
            || p.ends_with(" bool")
    };
    params.iter().filter(is_bool).count()
}

fn first_deep_line(source: &Source, f: &functions::Function) -> Option<usize> {
    let base = source.lines[f.start].indent;
    let unit = source.lang.indent_unit();
    source.lines[f.start + 1..=f.end]
        .iter()
        .find(|l| l.added && !l.comment && !l.raw.is_empty() && l.indent.saturating_sub(base) / unit > MAX_NESTING + 1)
        .map(|l| l.no)
}

fn function_findings(source: &Source) -> Vec<Finding> {
    let mut out = Vec::new();
    let mut add = |line: usize, rule: &'static str, message: String| {
        out.push(Finding::new(&source.path, line, rule, Severity::Warning, &message));
    };
    for f in functions::find(source) {
        let head = &source.lines[f.start];
        let added = source.lines[f.start..=f.end].iter().filter(|l| l.added).count();
        if added == 0 {
            continue;
        }
        let name = &f.name;
        if head.added && f.params.len() > MAX_PARAMS {
            add(
                head.no,
                "too-many-params",
                format!(
                    "`{name}` takes {} parameters (limit {MAX_PARAMS}); group them in a struct",
                    f.params.len()
                ),
            );
        }
        if head.added && bool_params(&f.params) >= 2 {
            add(
                head.no,
                "bool-flags",
                format!("`{name}` takes several bool flags; use an enum or separate functions"),
            );
        }
        if head.test {
            continue;
        }
        if added > MAX_FUNCTION_LINES {
            add(
                head.no,
                "long-function",
                format!("`{name}` adds {added} lines (limit {MAX_FUNCTION_LINES}); split it"),
            );
        }
        if let Some(no) = first_deep_line(source, &f) {
            add(
                no,
                "deep-nesting",
                format!("nesting deeper than {MAX_NESTING} levels in `{name}`; use early returns or helpers"),
            );
        }
    }
    out
}

/// Every rule's findings for the added lines of `source`.
#[must_use]
pub fn check(source: &Source) -> Vec<Finding> {
    let mut out = line_findings(source);
    if source.lang.is_code() {
        out.extend(function_findings(source));
        out.extend(new_file_size(source));
        out.extend(repeated_literals(source));
    }
    out.extend(conflict_markers(source));
    out
}

#[cfg(test)]
mod tests;
