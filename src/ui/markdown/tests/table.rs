use crate::ui::markdown::renderer::MarkdownRenderer;
use crate::ui::markdown::table::render_markdown_table_at_width;
use crate::ui::theme::Theme;
use unicode_width::UnicodeWidthStr;

#[test]
fn test_table_rendering() {
    let theme = Theme::default();
    let mut md = MarkdownRenderer::new();

    let chunk = "| Category | Details |\n|---|---|\n| Architecture | Linear Loop |\n\n";
    let out = md.render_token(chunk, &theme);
    assert!(out.contains("Category"));
    assert!(out.contains("Details"));
    assert!(out.contains("Architecture"));
    assert!(out.contains("Linear Loop"));
    assert!(out.contains('┌') || out.contains('+') || out.contains('-') || out.contains('│') || out.contains('╭'));
}

#[test]
fn table_renderer_uses_rounded_borders_and_respects_width() {
    let theme = Theme::default();
    let lines = vec![
        "| Name | Description |".to_string(),
        "| --- | --- |".to_string(),
        "| rho | a deliberately long table cell that wraps |".to_string(),
    ];
    let rendered = render_markdown_table_at_width(&lines, &theme, 36);
    let ansi = regex::Regex::new(r"\x1b\[[0-9;]*m").unwrap();
    let plain = ansi.replace_all(&rendered, "");
    assert!(plain.contains('╭'));
    assert!(plain.contains('╰'));
    assert!(plain.lines().all(|line| UnicodeWidthStr::width(line) <= 36));
}

#[test]
fn test_chunked_table_streaming() {
    let theme = Theme::default();
    let mut md = MarkdownRenderer::new();

    let t1 = md.render_token("| Name ", &theme);
    assert_eq!(t1, "");

    let t2 = md.render_token("| Role |\n", &theme);
    assert_eq!(t2, "");

    let t3 = md.render_token("|---|---|\n", &theme);
    assert_eq!(t3, "");

    let t4 = md.render_token("| Alice | Engineer |\n\n", &theme);
    assert!(t4.contains("Alice"));
    assert!(t4.contains("Engineer"));
    assert!(t4.contains('┌') || t4.contains('+') || t4.contains('-') || t4.contains('│') || t4.contains('╭'));
}

#[test]
fn test_pipe_text_without_divider_falls_back_to_text() {
    let theme = Theme::default();
    let mut md = MarkdownRenderer::new();

    let text = "| Just a line with pipes | not a real table\n\n";
    let out = md.render_token(text, &theme);
    assert!(out.contains("Just a line with pipes"));
    assert!(!out.contains('┌') && !out.contains('╭'));
}
