use crate::ui::markdown::elements::render_inline_elements;
use crate::ui::markdown::renderer::MarkdownRenderer;
use crate::ui::theme::Theme;

#[test]
fn test_bold_rendering() {
    let theme = Theme::default();
    let res = render_inline_elements("This is **important** text", &theme);
    assert!(!res.contains("**") && res.contains("important") && res.contains("\x1b[1m"));
}

#[test]
fn test_italic_rendering() {
    let theme = Theme::default();
    let res = render_inline_elements("This is *italic* text", &theme);
    assert!(res.contains("italic") && res.contains("\x1b[3m"));
}

#[test]
fn inline_code_complete_rendering() {
    let theme = Theme::default();
    let complete = render_inline_elements("Run `cargo test` now", &theme);
    assert!(complete.contains("cargo test") && !complete.contains('`') && complete.contains("\x1b[36m"));
}

#[test]
fn inline_code_streamed_rendering() {
    let theme = Theme::default();
    let mut markdown = MarkdownRenderer::new();
    let streamed = format!(
        "{}{}",
        markdown.render_token("Run `cargo", &theme),
        markdown.render_token(" test` now", &theme)
    );
    assert!(streamed.contains("cargo") && streamed.contains(" test") && !streamed.contains('`'));
}

#[test]
fn test_math_and_wildcard_asterisks_not_corrupted() {
    let theme = Theme::default();
    let res = render_inline_elements("formula: a * b * c and glob: *.rs", &theme);
    assert!(res.contains("a * b * c"));
    assert!(res.contains("*.rs"));
    assert!(!res.contains("\x1b[3m"));
}
