//! Syntax highlighting for generic tool outputs (`rg`, `fd`, `mcp`, `bash`, `lsp`, `worktree`,
//! JSON payloads, unified diffs, and fenced markdown code blocks).
//!
//! Outputs that already contain ANSI escape codes (such as colored `cargo test` stdout) are
//! returned unchanged so native tool colors are preserved.

use crate::ui::markdown::{CodeHighlighter, highlight_code_line, highlight_fenced_blocks_in, highlight_text};
use crate::ui::theme::Theme;

fn is_json_payload(trimmed: &str) -> bool {
    (trimmed.starts_with('{') && trimmed.ends_with('}')) || (trimmed.starts_with('[') && trimmed.ends_with(']'))
}

fn is_unified_diff(text: &str) -> bool {
    text.lines()
        .take(25)
        .any(|l| l.starts_with("diff --git ") || l.starts_with("@@ ") || l.starts_with("+++ "))
}

fn highlight_diff_line(line: &str, theme: &Theme) -> String {
    let (ok, err, accent, dim) = (theme.tool_ok, theme.tool_err, theme.highlight, theme.dimmed);
    let heading = accent.bold();
    if line.starts_with("diff --git ") || line.starts_with("+++ ") || line.starts_with("--- ") {
        format!("{heading}{line}{heading:#}")
    } else if line.starts_with("@@ ") {
        format!("{dim}{accent}{line}{accent:#}{dim:#}")
    } else if line.starts_with('+') {
        format!("{ok}{line}{ok:#}")
    } else if line.starts_with('-') {
        format!("{err}{line}{err:#}")
    } else {
        line.to_string()
    }
}

fn highlight_diff_output(text: &str, theme: &Theme) -> String {
    text.lines()
        .map(|line| highlight_diff_line(line, theme))
        .collect::<Vec<_>>()
        .join("\n")
}

fn parse_rg_row(line: &str) -> Option<(&str, char, &str)> {
    let digits = line.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    let sep = line.as_bytes().get(digits).copied()? as char;
    if sep != ':' && sep != '-' {
        return None;
    }
    Some((&line[..digits], sep, &line[digits + 1..]))
}

fn is_rg_heading(line: &str) -> bool {
    parse_rg_row(line).is_none() && !line.is_empty() && !line.starts_with('[') && !line.contains(" matches in ")
}

fn extension_of(path: &str) -> Option<String> {
    std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_string)
}

/// The language of the file whose matches `context` ends in.
fn rg_lang_after(context: &str) -> Option<String> {
    context
        .lines()
        .rev()
        .find(|line| is_rg_heading(line))
        .and_then(extension_of)
}

fn highlight_rg_output(text: &str, mut current_lang: Option<String>, theme: &Theme) -> String {
    let (accent, warn, dim) = (theme.highlight, theme.warning, theme.dimmed);
    let heading = accent.bold();
    let mut out = Vec::new();
    for line in text.lines() {
        if let Some((num, sep, code)) = parse_rg_row(line) {
            let num_s = if sep == ':' {
                format!("{warn}{num}{warn:#}{dim}:{dim:#}")
            } else {
                format!("{dim}{num}-{dim:#}")
            };
            let code_s = match current_lang.as_deref() {
                Some(ext) => highlight_code_line(code, Some(ext), theme),
                None => code.to_string(),
            };
            out.push(format!("{num_s}{code_s}"));
        } else if is_rg_heading(line) {
            current_lang = extension_of(line);
            out.push(format!("{heading}{line}{heading:#}"));
        } else {
            out.push(format!("{dim}{line}{dim:#}"));
        }
    }
    out.join("\n")
}

fn highlight_fd_output(text: &str, theme: &Theme) -> String {
    let (accent, dim) = (theme.highlight, theme.dimmed);
    let heading = accent.bold();
    text.lines()
        .map(|line| {
            if !line.starts_with(' ') && line.ends_with('/') {
                format!("{heading}{line}{heading:#}")
            } else if line.ends_with('/') {
                format!("{accent}{line}{accent:#}")
            } else if line.starts_with('[') {
                format!("{dim}{line}{dim:#}")
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// A highlighter for the ``` fence `context` leaves open, if it leaves one open.
fn open_fence_after(context: &str) -> Option<CodeHighlighter> {
    let mut open: Option<Option<&str>> = None;
    for line in context.lines() {
        if let Some(info) = line.trim_start().strip_prefix("```") {
            open = match open {
                Some(_) => None,
                None => Some(info.split_whitespace().next()),
            };
        }
    }
    open.map(CodeHighlighter::new)
}

fn highlight_diagnostic_line(line: &str, theme: &Theme) -> String {
    let (err, warn, accent) = (theme.tool_err, theme.warning, theme.highlight);
    let lower = line.to_ascii_lowercase();
    if lower.contains("error") {
        format!("{err}{line}{err:#}")
    } else if lower.contains("warning") {
        format!("{warn}{line}{warn:#}")
    } else if line.contains(':')
        && line
            .split(':')
            .nth(1)
            .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
    {
        format!("{accent}{line}{accent:#}")
    } else {
        line.to_string()
    }
}

/// The byte offset where line `line` of `text` starts, or its end when it has fewer lines.
pub(crate) fn line_start(text: &str, line: usize) -> usize {
    match line.checked_sub(1) {
        None => 0,
        Some(newlines) => text
            .match_indices('\n')
            .nth(newlines)
            .map_or(text.len(), |(i, _)| i + 1),
    }
}

/// Applies lightweight, ANSI-16 syntax highlighting to the lines of a plain-text tool output from
/// `first_line` on, reading the lines before it only for the kind of output and the file or fence they leave open.
pub fn highlight_tool_output(tool_name: &str, text: &str, first_line: usize, theme: &Theme) -> String {
    let (context, shown) = text.split_at(line_start(text, first_line));
    if text.is_empty() || text.contains('\x1b') {
        return shown.to_string();
    }
    if tool_name == "rg" {
        return highlight_rg_output(shown, rg_lang_after(context), theme);
    }
    if tool_name == "fd" {
        return highlight_fd_output(shown, theme);
    }
    if is_json_payload(text.trim()) {
        return highlight_text(shown, Some("json"), theme);
    }
    if is_unified_diff(text) {
        return highlight_diff_output(shown, theme);
    }
    if text.contains("```") {
        return highlight_fenced_blocks_in(shown, open_fence_after(context), theme);
    }
    if tool_name == "lsp" || tool_name == "worktree" {
        return shown
            .lines()
            .map(|l| highlight_diagnostic_line(l, theme))
            .collect::<Vec<_>>()
            .join("\n");
    }
    shown.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlighting_from_a_line_matches_the_same_lines_of_the_whole_output() {
        let theme = Theme::default();
        let cases = [
            (
                "rg",
                "src/a.rs\n1:fn a() {}\n2-let b = 1;\n\nsrc/b.py\n3:def c(): pass\n4:x = 1",
            ),
            ("fd", "src/\n  a.rs\n  sub/\n[3 more]\nREADME.md"),
            (
                "bash",
                "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1 +1 @@\n-old\n+new\n same",
            ),
            (
                "bash",
                "intro\n```rust\nfn main() {\n    let x = 1;\n}\n```\nafter\n```sh\necho hi",
            ),
            ("lsp", "src/a.rs:12\nerror: bad\nwarning: meh\nplain"),
            ("mcp", "{\n  \"a\": [\n    1,\n    true\n  ],\n  \"b\": \"c\"\n}"),
            ("bash", "\x1b[32mok\x1b[0m\nline two\r\nline three"),
            ("bash", "plain one\nplain two\nplain three"),
        ];
        for (tool, text) in cases {
            let whole = highlight_tool_output(tool, text, 0, &theme);
            let whole_lines: Vec<&str> = whole.split('\n').collect();
            for first in 0..=text.split('\n').count() {
                let from = highlight_tool_output(tool, text, first, &theme);
                let expected = whole_lines[first.min(whole_lines.len())..].join("\n");
                assert_eq!(from, expected, "{tool} from line {first} of {text:?}");
            }
        }
    }

    #[test]
    fn preserves_existing_ansi_output() {
        let colored = "\x1b[32mok\x1b[0m";
        assert_eq!(highlight_tool_output("bash", colored, 0, &Theme::default()), colored);
    }

    #[test]
    fn highlights_json_and_unified_diffs_and_rg_output() {
        let theme = Theme::default();
        let json = highlight_tool_output("mcp", "{\"status\": \"ok\", \"count\": 3}", 0, &theme);
        assert!(json.contains("\x1b["));
        assert_eq!(
            crate::ui::block::strip_ansi(&json),
            "{\"status\": \"ok\", \"count\": 3}"
        );

        let diff = highlight_tool_output("worktree", "diff --git a/x b/x\n@@ -1 +1 @@\n-old\n+new", 0, &theme);
        assert!(diff.contains("\x1b["));
        assert_eq!(
            crate::ui::block::strip_ansi(&diff),
            "diff --git a/x b/x\n@@ -1 +1 @@\n-old\n+new"
        );

        let rg = highlight_tool_output("rg", "src/lib.rs\n12:pub fn run() {}", 0, &theme);
        assert!(rg.contains("\x1b["));
        assert_eq!(crate::ui::block::strip_ansi(&rg), "src/lib.rs\n12:pub fn run() {}");
    }
}
