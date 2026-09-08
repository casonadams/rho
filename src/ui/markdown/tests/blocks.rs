use crate::ui::markdown::CodeHighlighter;
use crate::ui::markdown::highlight::highlight_code_line;
use crate::ui::markdown::renderer::MarkdownRenderer;
use crate::ui::theme::Theme;

#[test]
fn test_mermaid_rendering() {
    let theme = Theme::default();
    let mut md = MarkdownRenderer::new();

    let chunk = "```mermaid\ngraph TD\n  A[Start] --> B[End]\n```\n\n";
    let out = md.render_token(chunk, &theme);
    assert!(out.contains("mermaid diagram"));
}

#[test]
fn mermaid_lines_clip_to_renderer_width() {
    let theme = Theme::default();
    let source = "graph LR\n  A[Build] --> B[Test] --> C[Package] --> D[Deploy Stage] --> E[Deploy Prod]";
    let unclipped = crate::ui::markdown::render_mermaid_block(source, &theme, 0);
    let widest = unclipped
        .lines()
        .map(crate::ui::interactive::footer::visible_width)
        .max()
        .unwrap_or(0);
    assert!(widest > 80, "probe diagram must exceed the clip width: {widest}");

    let clipped = crate::ui::markdown::render_mermaid_block(source, &theme, 60);
    for line in clipped.lines() {
        assert!(
            crate::ui::interactive::footer::visible_width(line) <= 60,
            "line too wide: {line:?}"
        );
    }
    assert!(clipped.contains("┌"));
}

#[test]
fn mermaid_parse_fallback_lines_clip_too() {
    let theme = Theme::default();
    let long_source = "sequenceDiagram\n  ".to_string() + &"x".repeat(120);
    let out = crate::ui::markdown::render_mermaid_block(&long_source, &theme, 40);
    for line in out.lines() {
        assert!(
            crate::ui::interactive::footer::visible_width(line) <= 40,
            "fallback line too wide: {line:?}"
        );
    }
    assert!(out.contains("│"));
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

    let streaming = md.render_token("```rust\nlet x = 1;\nlet y = 2;", &theme) + &md.flush(&theme);

    let mut reference = MarkdownRenderer::new();
    let terminated = reference.render_token("```rust\nlet x = 1;\nlet y = 2;\n", &theme);

    assert_eq!(streaming, terminated);
}

#[test]
fn test_header_rendering() {
    let theme = Theme::default();
    let mut md = MarkdownRenderer::new();
    let h1 = md.render_line("# Main Title", &theme);
    assert!(h1.contains("Main Title"));
    let h4 = md.render_line("#### Level 4 Heading", &theme);
    assert!(h4.contains("####") && h4.contains("Level 4 Heading"));
}

#[test]
fn test_list_rendering() {
    let theme = Theme::default();
    let mut md = MarkdownRenderer::new();
    let bullet = md.render_line("- first item", &theme);
    assert!(bullet.contains("first item") && bullet.contains('•'));
    let num = md.render_line("1. First step", &theme);
    assert!(num.contains("1.") && num.contains("First step"));
    let indented_bullet = md.render_line("  - nested item", &theme);
    assert!(indented_bullet.contains('•') && indented_bullet.starts_with("  "));
    let indented_num = md.render_line("   1. nested step", &theme);
    assert!(indented_num.contains("1.") && indented_num.starts_with("   "));
}

#[test]
fn test_quote_rendering() {
    let theme = Theme::default();
    let mut md = MarkdownRenderer::new();
    let quote = md.render_line("  > quoted text", &theme);
    assert!(quote.contains('│') && quote.starts_with("  "));
    let empty_quote = md.render_line(">", &theme);
    assert!(empty_quote.contains('│'));
    let nested_quote = md.render_line(">> nested quote", &theme);
    assert_eq!(nested_quote.matches('│').count(), 2);
    assert!(nested_quote.contains("nested quote"));
}

fn assert_task_line(line: &str, marker: &str, text: &str) {
    assert!(line.contains(marker) && line.contains(text) && !line.contains('•'));
}

#[test]
fn test_task_list_rendering() {
    let theme = Theme::default();
    let mut md = MarkdownRenderer::new();

    let unchecked = md.render_line("- [ ] incomplete task", &theme);
    assert_task_line(&unchecked, "[ ]", "incomplete task");

    let checked = md.render_line("- [x] completed task", &theme);
    assert_task_line(&checked, "[x]", "completed task");

    let indented = md.render_line("  * [ ] indented task", &theme);
    assert!(indented.starts_with("  "));
    assert_task_line(&indented, "[ ]", "indented task");

    let ordered = md.render_line("1. [ ] numbered task", &theme);
    assert!(ordered.contains("1.") && ordered.contains("[ ]"));
}

#[test]
fn test_horizontal_rule_rendering() {
    let theme = Theme::default();
    let mut md = MarkdownRenderer::new();

    let rule_dashes = md.render_line("---", &theme);
    assert!(rule_dashes.contains('─'));
    assert!(rule_dashes.chars().filter(|&c| c == '─').count() >= 40);

    let rule_stars = md.render_line("***", &theme);
    assert!(rule_stars.contains('─'));

    let rule_underscores = md.render_line("___", &theme);
    assert!(rule_underscores.contains('─'));
}
