//! Syntax highlighting for generic tool outputs (`rg`, `fd`, `mcp`, `bash`, `lsp`, `worktree`,
//! JSON payloads, unified diffs, and fenced markdown code blocks).
//!
//! Outputs that already contain ANSI escape codes (such as colored `cargo test` stdout) are
//! returned unchanged so native tool colors are preserved.

use crate::ui::markdown::CodeHighlighter;
use crate::ui::theme::Theme;

fn is_json_payload(trimmed: &str) -> bool {
    (trimmed.starts_with('{') && trimmed.ends_with('}')) || (trimmed.starts_with('[') && trimmed.ends_with(']'))
}

fn is_unified_diff(text: &str) -> bool {
    text.lines()
        .take(25)
        .any(|l| l.starts_with("diff --git ") || l.starts_with("@@ ") || l.starts_with("+++ "))
}

fn highlight_with_lang(text: &str, lang: &str, theme: &Theme) -> String {
    let mut highlighter = CodeHighlighter::new(Some(lang), theme);
    text.lines()
        .map(|line| highlighter.highlight_line(line, theme))
        .collect::<Vec<_>>()
        .join("\n")
}

fn highlight_diff_line(line: &str, theme: &Theme) -> String {
    let (ok, err, accent, dim) = (theme.tool_ok, theme.tool_err, theme.highlight, theme.dimmed);
    if line.starts_with("diff --git ") || line.starts_with("+++ ") || line.starts_with("--- ") {
        format!("\x1b[1m{accent}{line}{accent:#}\x1b[0m")
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

fn highlight_rg_output(text: &str, theme: &Theme) -> String {
    let (accent, warn, dim) = (theme.highlight, theme.warning, theme.dimmed);
    let mut current_lang: Option<String> = None;
    let mut out = Vec::new();
    for line in text.lines() {
        if let Some((num, sep, code)) = parse_rg_row(line) {
            let num_s = if sep == ':' {
                format!("{warn}{num}{warn:#}{dim}:{dim:#}")
            } else {
                format!("{dim}{num}-{dim:#}")
            };
            let code_s = match current_lang.as_deref() {
                Some(ext) => crate::ui::markdown::highlight_code_line(code, Some(ext), theme),
                None => code.to_string(),
            };
            out.push(format!("{num_s}{code_s}"));
        } else if !line.is_empty() && !line.starts_with('[') && !line.contains(" matches in ") {
            current_lang = std::path::Path::new(line)
                .extension()
                .and_then(|e| e.to_str())
                .map(str::to_string);
            out.push(format!("\x1b[1m{accent}{line}{accent:#}\x1b[0m"));
        } else {
            out.push(format!("{dim}{line}{dim:#}"));
        }
    }
    out.join("\n")
}

fn highlight_fd_output(text: &str, theme: &Theme) -> String {
    let (accent, dim) = (theme.highlight, theme.dimmed);
    text.lines()
        .map(|line| {
            if !line.starts_with(' ') && line.ends_with('/') {
                format!("\x1b[1m{accent}{line}{accent:#}\x1b[0m")
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

fn highlight_fenced_blocks(text: &str, theme: &Theme) -> String {
    let dim = theme.dimmed;
    let mut active: Option<CodeHighlighter> = None;
    let mut out = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("```") {
            if active.is_some() {
                active = None;
            } else {
                let lang = rest.split_whitespace().next().filter(|s| !s.is_empty());
                active = Some(CodeHighlighter::new(lang, theme));
            }
            out.push(format!("{dim}{line}{dim:#}"));
        } else if let Some(hl) = active.as_mut() {
            out.push(hl.highlight_line(line, theme));
        } else {
            out.push(line.to_string());
        }
    }
    out.join("\n")
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

/// Applies lightweight, ANSI-16 syntax highlighting to plain-text tool outputs.
pub fn highlight_tool_output(tool_name: &str, text: &str, theme: &Theme) -> String {
    if text.is_empty() || text.contains('\x1b') {
        return text.to_string();
    }
    let trimmed = text.trim();
    if tool_name == "rg" {
        return highlight_rg_output(text, theme);
    }
    if tool_name == "fd" {
        return highlight_fd_output(text, theme);
    }
    if is_json_payload(trimmed) {
        return highlight_with_lang(text, "json", theme);
    }
    if is_unified_diff(text) {
        return highlight_diff_output(text, theme);
    }
    if text.contains("```") {
        return highlight_fenced_blocks(text, theme);
    }
    if tool_name == "lsp" || tool_name == "worktree" {
        return text
            .lines()
            .map(|l| highlight_diagnostic_line(l, theme))
            .collect::<Vec<_>>()
            .join("\n");
    }
    text.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_existing_ansi_output() {
        let colored = "\x1b[32mok\x1b[0m";
        assert_eq!(highlight_tool_output("bash", colored, &Theme::default()), colored);
    }

    #[test]
    fn highlights_json_and_unified_diffs_and_rg_output() {
        let theme = Theme::default();
        let json = highlight_tool_output("mcp", "{\"status\": \"ok\", \"count\": 3}", &theme);
        assert!(json.contains("\x1b["));
        assert_eq!(
            strip_ansi_escapes::strip_str(&json),
            "{\"status\": \"ok\", \"count\": 3}"
        );

        let diff = highlight_tool_output("worktree", "diff --git a/x b/x\n@@ -1 +1 @@\n-old\n+new", &theme);
        assert!(diff.contains("\x1b["));
        assert_eq!(
            strip_ansi_escapes::strip_str(&diff),
            "diff --git a/x b/x\n@@ -1 +1 @@\n-old\n+new"
        );

        let rg = highlight_tool_output("rg", "src/lib.rs\n12:pub fn run() {}", &theme);
        assert!(rg.contains("\x1b["));
        assert_eq!(strip_ansi_escapes::strip_str(&rg), "src/lib.rs\n12:pub fn run() {}");
    }
}
