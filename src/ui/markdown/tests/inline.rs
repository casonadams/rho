use crate::ui::markdown::elements::render_inline_elements;
use crate::ui::markdown::renderer::MarkdownRenderer;
use crate::ui::theme::Theme;

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
fn inline_code_hides_backticks_in_complete_and_streamed_text() {
    let theme = Theme::default();
    let complete = render_inline_elements("Run `cargo test` now", &theme);
    assert!(complete.contains("cargo test"));
    assert!(!complete.contains('`'));
    assert!(complete.contains("\x1b[36m"));

    let mut markdown = MarkdownRenderer::new();
    let streamed = format!(
        "{}{}",
        markdown.render_token("Run `cargo", &theme),
        markdown.render_token(" test` now", &theme)
    );
    assert!(streamed.contains("cargo"));
    assert!(streamed.contains(" test"));
    assert!(!streamed.contains('`'));
}

#[test]
fn test_math_and_wildcard_asterisks_not_corrupted() {
    let theme = Theme::default();
    let res = render_inline_elements("formula: a * b * c and glob: *.rs", &theme);
    assert!(res.contains("a * b * c"));
    assert!(res.contains("*.rs"));
    assert!(!res.contains("\x1b[3m"));
}
