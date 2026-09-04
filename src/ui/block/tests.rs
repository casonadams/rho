use super::wrap::{sgr_resets_background, visible_width};
use super::*;
use anstyle::{AnsiColor, Color};

fn background() -> Style {
    Style::new().bg_color(Some(Color::Ansi(AnsiColor::Black)))
}

#[test]
fn plain_blocks_wrap_and_pad_to_the_requested_width() {
    let rendered = BlockFormat::new(background(), 6)
        .with_vertical_padding()
        .render_plain("abcdefghij");
    let lines: Vec<&str> = rendered.lines().collect();
    assert_eq!(lines.len(), 4);
    assert!(lines.iter().all(|line| visible_width(line) == 6));
    assert!(rendered.contains("abcdef"));
    assert!(rendered.contains("ghij"));
}

#[test]
fn styled_blocks_wrap_to_full_width_and_preserve_active_color() {
    let rendered = BlockFormat::new(background(), 8).render_styled("\x1b[36mabcdefghijkl\x1b[0m");
    let lines: Vec<&str> = rendered.lines().collect();
    assert_eq!(lines.len(), 2);
    assert!(lines.iter().all(|line| visible_width(line) == 8));
    assert!(lines[1].contains("\x1b[36mijkl"));
}

#[test]
fn styled_content_keeps_its_background_after_an_inner_reset() {
    let rendered = BlockFormat::new(background(), 20).render_line("\x1b[31merror\x1b[0m text");
    assert_eq!(visible_width(&rendered), 20);
    assert!(rendered.contains("\x1b[0m\x1b[40m text"));
    assert!(rendered.ends_with("\x1b[0m"));
}

#[test]
fn multiline_styled_blocks_preserve_background_across_resets_and_blank_lines() {
    let content = "\x1b[1m\x1b[31mbold red\x1b[0m\n\n\x1b[32m+ line 2\x1b[0m extra";
    let rendered = BlockFormat::new(background(), 24)
        .with_vertical_padding()
        .render_styled(content);
    let lines: Vec<&str> = rendered.lines().collect();
    assert_eq!(lines.len(), 5);
    for line in &lines {
        assert_eq!(visible_width(line), 24);
        assert!(line.starts_with("\x1b[40m"));
    }
    assert!(rendered.contains("\x1b[0m\x1b[40m extra"));
}

#[test]
fn compound_and_color_resets_are_detected_correctly() {
    assert!(sgr_resets_background("\x1b[m"));
    assert!(sgr_resets_background("\x1b[0m"));
    assert!(sgr_resets_background("\x1b[49m"));
    assert!(sgr_resets_background("\x1b[0;31m"));
    assert!(sgr_resets_background("\x1b[31;0m"));
    assert!(!sgr_resets_background("\x1b[31m"));
    assert!(!sgr_resets_background("\x1b[1;32m"));
    assert!(!sgr_resets_background("\x1b[38;2;255;0;0m"));
    assert!(!sgr_resets_background("\x1b[38;5;0m"));
}
