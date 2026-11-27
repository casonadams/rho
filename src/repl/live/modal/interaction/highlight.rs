//! Syntax highlighting and semantic styling for permission / Guard modals and question modals.

use crate::ui::markdown::{highlight_fenced_blocks, highlight_text};
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
    let (accent, warn, err) = (theme.highlight.bold(), theme.warning.bold(), theme.tool_err.bold());
    let risk_text = theme.tool_err;
    if let Some(tool) = line.strip_prefix("Tool: ") {
        format!("{dim}Tool:{dim:#} {accent}{tool}{accent:#}")
    } else if let Some(action) = line.strip_prefix("Action: ") {
        format!("{accent}Action:{accent:#} {action}")
    } else if let Some(notice) = line.strip_prefix("Notice: ") {
        format!("{warn}Notice:{warn:#} {notice}")
    } else if let Some(risk) = line.strip_prefix("Risk: ") {
        format!("{err}Risk:{err:#} {risk_text}{risk}{risk_text:#}")
    } else {
        line.to_string()
    }
}

fn highlight_code_block(code: &str, lang: Option<&str>, theme: &Theme) -> String {
    let clean = crate::ui::block::strip_ansi(code);
    let trimmed = clean.trim_end_matches('\n');
    let Some(lang) = lang else {
        return format!("{trimmed}\n");
    };
    format!("{}\n", highlight_text(trimmed, Some(lang), theme))
}

/// Enhances `Permission Required` (including `[Guard]`) modals and any interactive modal containing
pub(super) fn style_permission_body(title: &str, body: String, theme: &Theme) -> String {
    if !theme.syntax_highlight_tools {
        return body;
    }
    if !title.starts_with("Permission Required") {
        return if body.contains("```") {
            highlight_fenced_blocks(&body, theme)
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
    let clean_code = crate::ui::block::strip_ansi(code);
    let lang = detect_code_lang(tool, &clean_code);
    let styled_code = highlight_code_block(code, lang, theme);
    format!("{styled_header}\n\n{styled_code}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlights_bash_guard_modal_body_and_preserves_text() {
        let theme = Theme {
            syntax_highlight_tools: true,
            ..Theme::default()
        };
        let raw = "Tool: bash\nNotice: Remote git push\nRisk: High risk\n\n\x1b[2mgit push origin main\x1b[0m\n";
        let styled = style_permission_body("Permission Required [Guard]", raw.to_string(), &theme);
        assert!(styled.contains("\x1b["), "expected ANSI highlighting: {styled}");
        let plain = crate::ui::block::strip_ansi(&styled);
        assert!(plain.contains("Tool: bash"));
        assert!(plain.contains("Notice: Remote git push"));
        assert!(plain.contains("Risk: High risk"));
        assert!(plain.contains("git push origin main"));
    }

    #[test]
    fn highlights_fenced_code_in_question_modals() {
        let theme = Theme {
            syntax_highlight_tools: true,
            ..Theme::default()
        };
        let raw = "Run this query?\n```sql\nSELECT * FROM users;\n```".to_string();
        let styled = style_permission_body("Confirm", raw, &theme);
        assert!(styled.contains("\x1b["));
        assert!(crate::ui::block::strip_ansi(&styled).contains("SELECT * FROM users;"));
    }

    #[test]
    fn leaves_plain_non_permission_modals_untouched() {
        let raw = "Some body\n\nfoo".to_string();
        assert_eq!(style_permission_body("Settings", raw.clone(), &Theme::default()), raw);
    }
}
