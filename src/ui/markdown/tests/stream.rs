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
fn test_split_ordered_list_marker() {
    let theme = Theme::default();
    let mut md = MarkdownRenderer::new();

    let t1 = md.render_token("1", &theme);
    let t2 = md.render_token(".", &theme);
    let t3 = md.render_token(" First step\n", &theme);
    let flushed = md.flush(&theme);
    let full = format!("{t1}{t2}{t3}{flushed}");
    assert!(full.contains("1."));
    assert!(full.contains("First step"));
}

#[test]
fn test_multi_digit_ordered_list_marker() {
    let theme = Theme::default();
    let mut md = MarkdownRenderer::new();

    let t1 = md.render_token("10", &theme);
    let t2 = md.render_token(".", &theme);
    let t3 = md.render_token(" Tenth step\n", &theme);
    let flushed = md.flush(&theme);
    let full = format!("{t1}{t2}{t3}{flushed}");
    assert!(full.contains("10."));
    assert!(full.contains("Tenth step"));
}

#[test]
fn test_stream_split_bold_asterisks() {
    let theme = Theme::default();
    let mut md = MarkdownRenderer::new();

    let t1 = md.render_token("This is *", &theme);
    let t2 = md.render_token("*bold** text\n", &theme);
    let full = format!("{t1}{t2}");
    assert!(full.contains("bold"));
    assert!(full.contains("\x1b[1m"));
}

#[test]
fn test_stream_math_asterisks_does_not_toggle_italic() {
    let theme = Theme::default();
    let mut md = MarkdownRenderer::new();

    let t1 = md.render_token("Math: 3 * ", &theme);
    let t2 = md.render_token("4 = 12\n", &theme);
    let full = format!("{t1}{t2}");
    assert!(full.contains("3 * 4 = 12"));
    assert!(!full.contains("\x1b[3m"));
}

#[test]
fn test_unbuffered_transition_preserves_buffered_prefix() {
    let theme = Theme::default();
    let mut md = MarkdownRenderer::new();

    let t1 = md.render_token("-", &theme);
    let t2 = md.render_token("-flag\n", &theme);
    let full = format!("{t1}{t2}");
    assert!(full.contains("--flag"));
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
