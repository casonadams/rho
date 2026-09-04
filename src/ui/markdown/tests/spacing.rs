use crate::ui::markdown::renderer::MarkdownRenderer;
use crate::ui::theme::Theme;

#[test]
fn test_leading_blank_lines_are_stripped() {
    let theme = Theme::default();
    let mut md = MarkdownRenderer::new();

    let out = md.render_token("\n\n\nHello world\n", &theme);
    assert_eq!(out, "Hello world\n");
}

#[test]
fn test_consecutive_blank_lines_are_collapsed() {
    let theme = Theme::default();
    let mut md = MarkdownRenderer::new();

    let text = "Paragraph one\n\n\n\n\nParagraph two\n";
    let out = md.render_token(text, &theme);
    assert_eq!(out, "Paragraph one\n\nParagraph two\n");
}

#[test]
fn test_header_at_start_has_no_leading_blank_line() {
    let theme = Theme::default();
    let mut md = MarkdownRenderer::new();

    let out = md.render_token("# Introduction\nSome text\n", &theme);
    assert!(!out.starts_with('\n'));
    assert!(out.contains("Introduction"));
}

#[test]
fn test_header_preceded_by_prose_inserts_single_blank_line() {
    let theme = Theme::default();
    let mut md = MarkdownRenderer::new();

    let text = "First paragraph\n# Heading\nSecond paragraph\n";
    let out = md.render_token(text, &theme);
    assert!(out.contains("First paragraph\n\n"));
    assert!(out.contains("Heading"));
}

#[test]
fn test_header_preceded_by_blank_line_does_not_double_space() {
    let theme = Theme::default();
    let mut md = MarkdownRenderer::new();

    let text = "First paragraph\n\n# Heading\nSecond paragraph\n";
    let out = md.render_token(text, &theme);
    assert!(!out.contains("\n\n\n"));
    assert!(out.contains("First paragraph\n\n"));
}

#[test]
fn test_code_fence_spacing_is_normalized() {
    let theme = Theme::default();
    let mut md = MarkdownRenderer::new();

    let text = "Intro text\n```rust\nlet x = 1;\n```\nOutro text\n";
    let out = md.render_token(text, &theme);
    assert!(out.contains("Intro text\n\n"));
}

#[test]
fn test_trailing_blank_lines_are_stripped() {
    let theme = Theme::default();
    let mut md = MarkdownRenderer::new();

    let text = "Paragraph one\n\n\n\n";
    let out = md.render_token(text, &theme);
    let flushed = md.flush(&theme);
    let full = format!("{out}{flushed}");
    assert_eq!(full, "Paragraph one\n");
}
