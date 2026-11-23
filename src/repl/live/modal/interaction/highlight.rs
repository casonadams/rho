//! Syntax highlighting and semantic styling for permission / Guard modals and question modals.

use crate::ui::markdown::CodeHighlighter;
use crate::ui::theme::Theme;

fn detect_code_lang(tool: &str, code: &str) -> Option<&'static str> {
    if tool == "bash" {
        return Some("sh");
    }
    let trimmed = code.trim_start();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return Some("json");
    }
    let upper = trimmed.to_ascii_uppercase();
    if upper.starts_with("SELECT ")
        || upper.starts_with("INSERT ")
        || upper.starts_with("UPDATE ")
        || upper.starts_with("DELETE ")
        || upper.starts_with("CREATE ")
        || upper.starts_with("ALTER ")
        || upper.starts_with("DROP ")
    {
        return Some("sql");
    }
    None
}

fn style_header_line(line: &str, theme: &Theme) -> String {
    let dim = theme.dimmed;
    let accent = theme.highlight;
    let warn = theme.warning;
    let err = theme.tool_err;
    if let Some(tool) = line.strip_prefix("Tool: ") {
        format!("{dim}Tool:{dim:#} \x1b[1m{accent}{tool}{accent:#}\x1b[0m")
    } else if let Some(action) = line.strip_prefix("Action: ") {
        format!("\x1b[1m{accent}Action:{accent:#}\x1b[0m {action}")
    } else if let Some(notice) = line.strip_prefix("Notice: ") {
        format!("\x1b[1m{warn}Notice:{warn:#}\x1b[0m {notice}")
    } else if let Some(risk) = line.strip_prefix("Risk: ") {
        format!("\x1b[1m{err}Risk:{err:#}\x1b[0m {err}{risk}{err:#}")
    } else {
        line.to_string()
    }
}

fn highlight_code_block(code: &str, lang: Option<&str>, theme: &Theme) -> String {
    let clean = strip_ansi_escapes::strip_str(code);
    let trimmed = clean.trim_end_matches('\n');
    let Some(lang) = lang else {
        return format!("{trimmed}\n");
    };
    let mut highlighter = CodeHighlighter::new(Some(lang), theme);
    let mut out = String::new();
    for line in trimmed.lines() {
        out.push_str(&highlighter.highlight_line(line, theme));
        out.push('\n');
    }
    out
}

fn highlight_fenced_modal_body(body: &str, theme: &Theme) -> String {
    let dim = theme.dimmed;
    let mut active: Option<CodeHighlighter<'_>> = None;
    let mut out = Vec::new();
    for line in body.lines() {
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

/// Enhances `Permission Required` (including `[Guard]`) modals and any interactive modal containing
/// fenced code blocks with semantic label colors and `syntect` ANSI-16 syntax highlighting.
pub(super) fn style_permission_body(title: &str, body: String, theme: &Theme) -> String {
    if !title.starts_with("Permission Required") {
        return if body.contains("```") {
            highlight_fenced_modal_body(&body, theme)
        } else {
            body
        };
    }
    let Some((header, code)) = body.split_once("\n\n") else {
        return body;
    };
    let tool = header
        .lines()
        .find_map(|line| line.strip_prefix("Tool: "))
        .unwrap_or("")
        .trim();
    let styled_header = header
        .lines()
        .map(|line| style_header_line(line, theme))
        .collect::<Vec<_>>()
        .join("\n");
    let clean_code = strip_ansi_escapes::strip_str(code);
    let lang = detect_code_lang(tool, &clean_code);
    let styled_code = highlight_code_block(code, lang, theme);
    format!("{styled_header}\n\n{styled_code}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlights_bash_guard_modal_body_and_preserves_text() {
        let raw = "Tool: bash\nNotice: Remote git push\nRisk: High risk\n\n\x1b[2mgit push origin main\x1b[0m\n";
        let styled = style_permission_body("Permission Required [Guard]", raw.to_string(), &Theme::default());
        assert!(styled.contains("\x1b["), "expected ANSI highlighting: {styled}");
        let plain = strip_ansi_escapes::strip_str(&styled);
        assert!(plain.contains("Tool: bash"));
        assert!(plain.contains("Notice: Remote git push"));
        assert!(plain.contains("Risk: High risk"));
        assert!(plain.contains("git push origin main"));
    }

    #[test]
    fn highlights_fenced_code_in_question_modals() {
        let raw = "Run this query?\n```sql\nSELECT * FROM users;\n```".to_string();
        let styled = style_permission_body("Confirm", raw, &Theme::default());
        assert!(styled.contains("\x1b["));
        assert!(strip_ansi_escapes::strip_str(&styled).contains("SELECT * FROM users;"));
    }

    #[test]
    fn leaves_plain_non_permission_modals_untouched() {
        let raw = "Some body\n\nfoo".to_string();
        assert_eq!(style_permission_body("Settings", raw.clone(), &Theme::default()), raw);
    }
}
