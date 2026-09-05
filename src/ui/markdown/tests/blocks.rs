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

    let h4 = md.render_line("#### Level 4 Heading", &theme);
    assert!(h4.contains("####"));
    assert!(h4.contains("Level 4 Heading"));

    let indented_bullet = md.render_line("  - nested item", &theme);
    assert!(indented_bullet.contains('•'));
    assert!(indented_bullet.starts_with("  "));

    let indented_num = md.render_line("   1. nested step", &theme);
    assert!(indented_num.contains("1."));
    assert!(indented_num.starts_with("   "));

    let quote = md.render_line("  > quoted text", &theme);
    assert!(quote.contains('│'));
    assert!(quote.starts_with("  "));

    let empty_quote = md.render_line(">", &theme);
    assert!(empty_quote.contains('│'));

    let nested_quote = md.render_line(">> nested quote", &theme);
    assert_eq!(nested_quote.matches('│').count(), 2);
    assert!(nested_quote.contains("nested quote"));
}

#[test]
fn test_task_list_rendering() {
    let theme = Theme::default();
    let mut md = MarkdownRenderer::new();

    let unchecked = md.render_line("- [ ] incomplete task", &theme);
    assert!(unchecked.contains("[ ]"));
    assert!(unchecked.contains("incomplete task"));
    assert!(!unchecked.contains('•'));

    let checked = md.render_line("- [x] completed task", &theme);
    assert!(checked.contains("[x]"));
    assert!(checked.contains("completed task"));
    assert!(!checked.contains('•'));

    let indented_task = md.render_line("  * [ ] indented task", &theme);
    assert!(indented_task.starts_with("  "));
    assert!(indented_task.contains("[ ]"));
    assert!(!indented_task.contains('•'));

    let ordered_task = md.render_line("1. [ ] numbered task", &theme);
    assert!(ordered_task.contains("1."));
    assert!(ordered_task.contains("[ ]"));
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
