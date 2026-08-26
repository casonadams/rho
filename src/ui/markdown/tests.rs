//! Tests for the `ui::markdown` module.

use super::elements::render_inline_elements;
use super::highlight::highlight_code_line;
use super::renderer::MarkdownRenderer;
use crate::ui::theme::Theme;

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
    assert!(out.contains('┌') || out.contains('+') || out.contains('-') || out.contains('│'));
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
    assert!(t4.contains('┌') || t4.contains('+') || t4.contains('-') || t4.contains('│'));
}

#[test]
fn test_pipe_text_without_divider_falls_back_to_text() {
    let theme = Theme::default();
    let mut md = MarkdownRenderer::new();

    let text = "| Just a line with pipes | not a real table\n\n";
    let out = md.render_token(text, &theme);
    assert!(out.contains("Just a line with pipes"));
    assert!(!out.contains('┌'));
}

#[test]
fn test_stream_prose_word_by_word() {
    let theme = Theme::default();
    let mut md = MarkdownRenderer::new();

    let token1 = md.render_token("Hello ", &theme);
    assert_eq!(token1, "Hello ");

    let token2 = md.render_token("world", &theme);
    assert_eq!(token2, "world");

    let flushed = md.flush(&theme);
    assert_eq!(flushed, "\n");
}

#[test]
fn test_mermaid_rendering() {
    let theme = Theme::default();
    let mut md = MarkdownRenderer::new();

    let chunk = "```mermaid\ngraph TD\n  A[Start] --> B[End]\n```\n\n";
    let out = md.render_token(chunk, &theme);
    assert!(out.contains("mermaid diagram"));
}

#[test]
fn test_bold_and_italic_rendering() {
    let theme = Theme::default();
    let res = render_inline_elements("This is **important** and *italic* text", &theme);
    assert!(!res.contains("**"));
    assert!(res.contains("important"));
    assert!(res.contains("\x1b[1m"));
    assert!(res.contains("italic"));
    assert!(res.contains("\x1b[3m"));
}

#[test]
fn test_code_block_has_no_background_color_patches() {
    let theme = Theme::default();
    let highlighted = highlight_code_line("let x = 42;", Some("rust"), &theme);
    // 24-bit background color escape is \x1b[48;2;...
    assert!(!highlighted.contains("\x1b[48;2;"));
    assert!(highlighted.contains("let"));
    assert!(highlighted.contains("42"));
}

#[test]
fn test_code_block_fences_open_and_close() {
    let theme = Theme::default();
    let mut md = MarkdownRenderer::new();

    let l1 = md.render_line("```rust", &theme);
    assert!(l1.contains("```rust"));

    let l2 = md.render_line("fn main() {}", &theme);
    assert!(l2.contains("fn"));

    let l3 = md.render_line("```", &theme);
    assert!(l3.contains("```"));
}

#[test]
fn test_header_rendering() {
    let theme = Theme::default();
    let mut md = MarkdownRenderer::new();

    let h1 = md.render_line("# Main Title", &theme);
    assert!(h1.contains("Main Title"));

    let bullet = md.render_line("- first item", &theme);
    assert!(bullet.contains("first item"));
    assert!(bullet.contains('•'));

    let num = md.render_line("1. First step", &theme);
    assert!(num.contains("1."));
    assert!(num.contains("First step"));
}

#[test]
fn test_flush_emits_newline_when_line_uncompleted() {
    let theme = Theme::default();
    let mut md = MarkdownRenderer::new();

    let token = md.render_token("Hello world", &theme);
    assert_eq!(token, "Hello world");

    let flushed = md.flush(&theme);
    assert_eq!(flushed, "\n");

    let second_flush = md.flush(&theme);
    assert_eq!(second_flush, "");
}

#[test]
fn test_flush_does_not_emit_redundant_newline_when_already_terminated() {
    let theme = Theme::default();
    let mut md = MarkdownRenderer::new();

    let token = md.render_token("Hello world\n", &theme);
    assert_eq!(token, "Hello world\n");

    let flushed = md.flush(&theme);
    assert_eq!(flushed, "");
}

#[test]
fn test_math_and_wildcard_asterisks_not_corrupted() {
    let theme = Theme::default();
    let res = render_inline_elements("formula: a * b * c and glob: *.rs", &theme);
    assert!(res.contains("a * b * c"));
    assert!(res.contains("*.rs"));
    assert!(!res.contains("\x1b[3m"));
}
