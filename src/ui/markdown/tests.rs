use crate::ui::markdown::highlight::highlight_code_line;
use crate::ui::markdown::renderer::MarkdownRenderer;
use crate::ui::markdown::{CodeHighlighter, render_inline_elements};
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
fn test_math_and_wildcard_asterisks_not_corrupted() {
    let theme = Theme::default();
    let res = render_inline_elements("formula: a * b * c and glob: *.rs", &theme);
    assert!(res.contains("a * b * c"));
    assert!(res.contains("*.rs"));
    assert!(!res.contains("\x1b[3m"));
}

#[test]
fn html_comment_and_inline_tags_preserved() {
    let theme = Theme::default();
    let res = render_inline_elements("<!-- AGENT_RUN_COMPLETE -->", &theme);
    assert!(res.contains("<!-- AGENT_RUN_COMPLETE -->"));
    let res_tag = render_inline_elements("<custom-tag>data</custom-tag>", &theme);
    assert!(res_tag.contains("<custom-tag>") && res_tag.contains("data") && res_tag.contains("</custom-tag>"));
}

#[test]
fn test_code_block_has_no_background_color_patches() {
    let theme = Theme::default();
    let highlighted = highlight_code_line("let x = 42;", Some("rust"), &theme);
    assert!(!highlighted.contains("\x1b[48;2;"));
    assert!(highlighted.contains("let"));
    assert!(highlighted.contains("42"));
}

#[test]
fn code_blocks_show_fences_instead_of_code_bars() {
    let theme = Theme::default();
    let mut md = MarkdownRenderer::new();

    let opening = md.render_line("```rust", &theme);
    assert!(opening.contains("```rust"));

    let code = md.render_line("fn main() {}", &theme);
    assert!(code.contains("fn"));
    assert!(!code.contains('│'));

    let closing = md.render_line("```", &theme);
    assert!(closing.contains("```"));
}

#[test]
fn multiline_comment_state_carries_across_streamed_code_lines() {
    let theme = Theme::default();
    let mut md = MarkdownRenderer::new();

    let out = md.render_token(
        "```rust\n/* unterminated comment\nstill inside\ncomment ends */\n```\n",
        &theme,
    );

    let mut carried = CodeHighlighter::new(Some("rust"), &theme);
    carried.highlight_line("/* unterminated comment", &theme);
    let carried_second = carried.highlight_line("still inside", &theme);
    let per_line = CodeHighlighter::new(Some("rust"), &theme).highlight_line("still inside", &theme);

    assert_ne!(
        carried_second, per_line,
        "input must keep a different style when comment state spans lines"
    );
    assert!(out.contains(&carried_second), "streamed output: {out:?}");
    assert!(!out.contains(&per_line), "streamed output: {out:?}");
}

#[test]
fn fence_language_change_re_resolves_the_highlighter() {
    let theme = Theme::default();
    let mut md = MarkdownRenderer::new();

    let out = md.render_token("```rust\nlet x = 1;\n```\n```python\nprint('x')\n```\n", &theme);

    let python_line = CodeHighlighter::new(Some("python"), &theme).highlight_line("print('x')", &theme);
    assert!(out.contains(&python_line), "streamed output: {out:?}");
}

#[test]
fn flushed_unclosed_fence_matches_newline_terminated_rendering() {
    let theme = Theme::default();
    let mut md = MarkdownRenderer::new();

    let streaming = format!("{}{}", md.render_token("```rust\nlet x = 1;", &theme), md.flush(&theme));

    let mut complete_md = MarkdownRenderer::new();
    let complete = complete_md.render_token("```rust\nlet x = 1;\n", &theme);

    assert_eq!(streaming, complete);
}

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

struct StreamCase {
    chunks: &'static [&'static str],
    held_chunks: usize,
    table: bool,
    contains: &'static [&'static str],
    absent: &'static [&'static str],
}

const STREAM_CASES: [StreamCase; 11] = [
    StreamCase {
        chunks: &["Run `cargo", " test` now"],
        held_chunks: 0,
        table: false,
        contains: &["cargo", " test"],
        absent: &["`"],
    },
    StreamCase {
        chunks: &["The response from the", " active task is complete.\n"],
        held_chunks: 0,
        table: false,
        contains: &["The response from the", " active task is complete."],
        absent: &[],
    },
    StreamCase {
        chunks: &["-", " cargo test --all-targets\n"],
        held_chunks: 1,
        table: false,
        contains: &["cargo test --all-targets"],
        absent: &[],
    },
    StreamCase {
        chunks: &["1", ".", " First step\n"],
        held_chunks: 0,
        table: false,
        contains: &["1.", "First step"],
        absent: &[],
    },
    StreamCase {
        chunks: &["10", ".", " Tenth step\n"],
        held_chunks: 0,
        table: false,
        contains: &["10.", "Tenth step"],
        absent: &[],
    },
    StreamCase {
        chunks: &["This is *", "*bold** text\n"],
        held_chunks: 0,
        table: false,
        contains: &["bold", "\x1b[1m"],
        absent: &[],
    },
    StreamCase {
        chunks: &["Math: 3 * ", "4 = 12\n"],
        held_chunks: 0,
        table: false,
        contains: &["3 * 4 = 12"],
        absent: &["\x1b[3m"],
    },
    StreamCase {
        chunks: &["This is *ital", "ic* text\n"],
        held_chunks: 0,
        table: false,
        contains: &["italic", "\x1b[3m"],
        absent: &["*"],
    },
    StreamCase {
        chunks: &["-", "-flag\n"],
        held_chunks: 0,
        table: false,
        contains: &["--flag"],
        absent: &[],
    },
    StreamCase {
        chunks: &["| Name ", "| Role |\n", "|---|---|\n", "| Alice | Engineer |\n\n"],
        held_chunks: 3,
        table: true,
        contains: &["Alice", "Engineer"],
        absent: &[],
    },
    StreamCase {
        chunks: &["| Just a line with pipes | not a real table\n\n"],
        held_chunks: 0,
        table: false,
        contains: &["Just a line with pipes"],
        absent: &["\u{250c}", "\u{256d}"],
    },
];

#[test]
fn streamed_chunks_render_split_markdown() {
    let theme = Theme::default();
    for case in STREAM_CASES {
        let mut md = MarkdownRenderer::new();
        let mut outputs: Vec<String> = case.chunks.iter().map(|chunk| md.render_token(chunk, &theme)).collect();
        outputs.push(md.flush(&theme));
        let out = outputs.concat();
        for output in &outputs[..case.held_chunks] {
            assert!(output.is_empty(), "chunk output {output:?} for {:?}", case.chunks);
        }
        if case.table {
            assert_table_contains(&out, case.contains);
        } else {
            for item in case.contains {
                assert!(out.contains(item), "{item:?} missing from {out:?}");
            }
        }
        for item in case.absent {
            assert!(!out.contains(item), "{item:?} found in {out:?}");
        }
    }
}

#[test]
fn streamed_line_prefix_is_emitted_before_the_newline() {
    let mut md = MarkdownRenderer::new();
    assert!(
        md.render_token("The response from the", &Theme::default())
            .contains("The response from the")
    );
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

fn has_table_border(out: &str) -> bool {
    out.chars().any(|c| matches!(c, '┌' | '+' | '-' | '│' | '╭'))
}

fn assert_table_contains(out: &str, items: &[&str]) {
    for item in items {
        assert!(out.contains(item));
    }
    assert!(has_table_border(out));
}

#[test]
fn test_table_rendering() {
    let theme = Theme::default();
    let mut md = MarkdownRenderer::new();

    let chunk = "| Category | Details |\n|---|---|\n| Architecture | Linear Loop |\n\n";
    let out = md.render_token(chunk, &theme);
    assert_table_contains(&out, &["Category", "Details", "Architecture", "Linear Loop"]);
}

#[test]
fn test_warmup_syntax_highlighter_completes() {
    super::warmup_syntax_highlighter();
    super::warmup_syntax_highlighter();
}
