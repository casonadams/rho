use crate::ui::markdown::renderer::MarkdownRenderer;
use crate::ui::theme::Theme;

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
fn test_streamed_line_suffix_before_newline_is_not_dropped() {
    let theme = Theme::default();
    let mut md = MarkdownRenderer::new();

    let first = md.render_token("The response from the", &theme);
    let second = md.render_token(" active task is complete.\n", &theme);

    assert!(first.contains("The response from the"));
    assert!(second.contains(" active task is complete."));
}

#[test]
fn test_split_list_marker_does_not_drop_item_text() {
    let theme = Theme::default();
    let mut md = MarkdownRenderer::new();

    let marker = md.render_token("-", &theme);
    let item = md.render_token(" cargo test --all-targets\n", &theme);

    assert!(marker.is_empty());
    assert!(item.contains("cargo test --all-targets"));
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
