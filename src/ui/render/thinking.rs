//! Thinking output: markdown rendered dim with a one-column margin, shared by
//! live streaming and the committed transcript block.

use super::stream::{StreamKind, StreamRenderer};
use crate::ui::theme::Theme;

pub(crate) fn format_thinking_block(thinking_text: &str, theme: &Theme, width: usize) -> String {
    let mut renderer = StreamRenderer::new(StreamKind::Thinking, theme);
    let mut out = String::from("\n");
    out.push_str(&renderer.push(&format!("{}\n", thinking_text.trim()), width, theme));
    out.push_str(&renderer.finish(theme));
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::block::strip_ansi;
    use test_case::test_case;

    fn stream(tokens: &[&str], width: usize, theme: &Theme) -> String {
        let mut renderer = StreamRenderer::new(StreamKind::Thinking, theme);
        let mut out = String::new();
        for token in tokens {
            out.push_str(&renderer.push(token, width, theme));
        }
        out.push_str(&renderer.finish(theme));
        out
    }

    #[test]
    fn block_is_dim_indented_with_leading_blank_line() {
        let theme = Theme::default();
        let block = format_thinking_block("analyzing the problem\nchecking tests", &theme, 80);
        let dim = theme.dimmed;
        assert!(block.starts_with('\n') && block.ends_with('\n'));
        assert!(block.contains(&format!(" {dim}analyzing the problem")));
        assert_eq!(strip_ansi(&block), "\n analyzing the problem\n checking tests\n");
    }

    #[test]
    fn block_wraps_on_word_boundaries_inside_margin() {
        let theme = Theme::default();
        let block = format_thinking_block("first second third fourth fifth", &theme, 15);
        let lines: Vec<String> = strip_ansi(&block)
            .trim_end()
            .lines()
            .skip(1)
            .map(String::from)
            .collect();
        assert_eq!(lines, [" first second", " third fourth", " fifth"]);
    }

    #[test]
    fn renders_markdown_instead_of_literal_markers() {
        let theme = Theme::default();
        let plain = strip_ansi(&format_thinking_block("**Plan** the `fix`\n\n- one\n- two", &theme, 80));
        assert!(!plain.contains("**") && !plain.contains('`'));
        assert!(plain.contains(" Plan the fix") && plain.contains("• one"));
    }

    #[test]
    fn bold_keeps_dim_after_reset() {
        let theme = Theme::default();
        let block = format_thinking_block("a **b** c", &theme, 80);
        let dim = theme.dimmed;
        assert!(block.contains(&format!("\x1b[0m{dim}")));
    }

    #[test_case(0; "unbounded")]
    #[test_case(12; "narrow")]
    #[test_case(80; "wide")]
    fn streamed_tokens_match_committed_block(width: usize) {
        let theme = Theme::default();
        let text = "Considering **options** here.\n\n- first item\n- second item\n\nDone thinking.";
        let tokens: Vec<String> = text.split_inclusive(' ').map(String::from).collect();
        let refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
        assert_eq!(
            format!("\n{}\n", strip_ansi(&stream(&refs, width, &theme))),
            strip_ansi(&format_thinking_block(text, &theme, width))
        );
    }

    #[test]
    fn finish_resets_state_for_the_next_block() {
        let theme = Theme::default();
        let mut renderer = StreamRenderer::new(StreamKind::Thinking, &theme);
        renderer.push("one\n", 80, &theme);
        renderer.finish(&theme);
        let second = strip_ansi(&format!(
            "{}{}",
            renderer.push("two\n", 80, &theme),
            renderer.finish(&theme)
        ));
        assert_eq!(second, " two\n");
    }
}
