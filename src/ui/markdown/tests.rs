use crate::ui::markdown::highlight::highlight_code_line;
use crate::ui::markdown::renderer::MarkdownRenderer;
use crate::ui::markdown::table::render_markdown_table_at_width;
use crate::ui::markdown::{CodeHighlighter, render_inline_elements, render_mermaid_block};
use crate::ui::theme::Theme;
use unicode_width::UnicodeWidthStr;

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

#[test]
fn html_comment_and_inline_tags_preserved() {
    let theme = Theme::default();
    let res = render_inline_elements("<!-- AGENT_RUN_COMPLETE -->", &theme);
    assert!(res.contains("<!-- AGENT_RUN_COMPLETE -->"));
    let res_tag = render_inline_elements("<custom-tag>data</custom-tag>", &theme);
    assert!(res_tag.contains("<custom-tag>") && res_tag.contains("data") && res_tag.contains("</custom-tag>"));
}

#[test]
fn test_mermaid_rendering() {
    let theme = Theme::default();
    let mut md = MarkdownRenderer::new();
    let mut out = String::new();
    for token in ["```", "mermaid\n", "graph LR\n  Input --> Process --> Output\n", "```"] {
        out.push_str(&md.render_token(token, &theme));
    }
    out.push_str(&md.flush(&theme));
    assert!(out.contains("Input"));
    assert!(!out.contains("```mermaid"));
    assert!(!out.contains("```"));
}

#[test]
fn mermaid_lines_clip_to_renderer_width() {
    let theme = Theme::default();
    let source = "graph LR\n  A[Build] --> B[Test] --> C[Package] --> D[Deploy Stage] --> E[Deploy Prod]";
    let unclipped = render_mermaid_block(source, &theme, 0);
    let widest = unclipped
        .lines()
        .map(crate::ui::interactive::footer::visible_width)
        .max()
        .unwrap_or(0);
    assert!(widest > 60, "probe diagram must exceed the clip width: {widest}");

    let clipped = render_mermaid_block(source, &theme, 60);
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
    let out = render_mermaid_block(&long_source, &theme, 40);
    for line in out.lines() {
        assert!(
            crate::ui::interactive::footer::visible_width(line) <= 40,
            "fallback line too wide: {line:?}"
        );
    }
    assert!(out.contains("```mermaid"));
    assert!(!out.contains('│'));
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
fn test_flowchart_linear_td() {
    let theme = Theme::default();
    let source = "graph TD\n  A[Start] --> B[Process] --> C[Done]";
    let rendered = render_mermaid_block(source, &theme, 0);

    assert!(rendered.contains("Start"));
    assert!(rendered.contains("Process"));
    assert!(rendered.contains("Done"));
    assert!(rendered.contains('┌') || rendered.contains('╭'));
    assert!(rendered.contains('▼'));

    let start_pos = rendered.find("Start").unwrap();
    let proc_pos = rendered.find("Process").unwrap();
    let done_pos = rendered.find("Done").unwrap();
    assert!(start_pos < proc_pos, "Start must precede Process in TD layout");
    assert!(proc_pos < done_pos, "Process must precede Done in TD layout");
}

#[test]
fn test_flowchart_branching_td() {
    let theme = Theme::default();
    let source = "graph TD\n  A[Input] --> B[Left Path]\n  A --> C[Right Path]";
    let rendered = render_mermaid_block(source, &theme, 0);

    assert!(rendered.contains("Input"));
    assert!(rendered.contains("Left Path"));
    assert!(rendered.contains("Right Path"));
    assert!(rendered.contains('▼'));
}

#[test]
fn test_flowchart_loop_routing() {
    let theme = Theme::default();
    let source = r#"
graph TD
  UserInput[User Input] --> AgentHarness[Agent Harness]
  AgentHarness --> ModelCall{Model Call}
  ModelCall -->|Tool Call| ExecuteTool[Execute Tool]
  ExecuteTool --> AgentHarness
  ModelCall -->|Response| RenderUI[Render to UI]
"#;
    let rendered = render_mermaid_block(source, &theme, 0);

    assert!(rendered.contains("User Input"));
    assert!(rendered.contains("Agent Harness"));
    assert!(rendered.contains("Model Call"));
    assert!(rendered.contains("Execute Tool"));
    assert!(rendered.contains("Render to UI"));

    for line in rendered.lines() {
        assert!(UnicodeWidthStr::width(line) < 120, "line too wide in loop: {line:?}");
    }
}

#[test]
fn test_flowchart_subgraphs() {
    let theme = Theme::default();
    let source = r#"
flowchart TD
  subgraph Cluster
    W1[Worker 1] --> W2[Worker 2]
  end
  Client[Client App] --> Cluster
"#;
    let rendered = render_mermaid_block(source, &theme, 0);

    assert!(rendered.contains("Cluster"));
    assert!(rendered.contains("Worker 1"));
    assert!(rendered.contains("Worker 2"));
    assert!(rendered.contains("Client App"));
}

#[test]
fn test_sequence_diagram() {
    let theme = Theme::default();
    let source = r#"
sequenceDiagram
  Alice->>Bob: Hello Bob
  Bob-->>Alice: Hi Alice
"#;
    let rendered = render_mermaid_block(source, &theme, 0);

    assert!(rendered.contains("Alice"));
    assert!(rendered.contains("Bob"));
    assert!(rendered.contains("Hello Bob"));
    assert!(rendered.contains("Hi Alice"));
    assert!(rendered.contains('│'));
    assert!(rendered.contains('►') || rendered.contains('>'));
}

#[test]
fn test_state_diagram() {
    let theme = Theme::default();
    let source = r#"
stateDiagram-v2
  [*] --> Idle
  Idle --> Processing: Event
  Processing --> [*]
"#;
    let rendered = render_mermaid_block(source, &theme, 0);

    assert!(rendered.contains("Idle"));
    assert!(rendered.contains("Processing"));
    assert!(rendered.contains("Event"));
}

#[test]
fn test_class_diagram() {
    let theme = Theme::default();
    let source = r#"
classDiagram
  class BankAccount {
    +String owner
    +deposit()
  }
"#;
    let rendered = render_mermaid_block(source, &theme, 0);

    assert!(rendered.contains("BankAccount"));
    assert!(rendered.contains("owner"));
    assert!(rendered.contains("deposit"));
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
fn table_renderer_uses_rounded_borders_and_respects_width() {
    let theme = Theme::default();
    let lines = vec![
        "| Name | Description |".to_string(),
        "| --- | --- |".to_string(),
        "| rho | a deliberately long table cell that wraps |".to_string(),
    ];
    let rendered = render_markdown_table_at_width(&lines, &theme, 36);
    let ansi = regex::Regex::new(r"\x1b\[[0-9;]*m").unwrap();
    let plain = ansi.replace_all(&rendered, "");
    assert!(plain.contains('╭'));
    assert!(plain.contains('╰'));
    assert!(plain.lines().all(|line| UnicodeWidthStr::width(line) <= 36));
}

#[test]
fn test_chunked_table_streaming() {
    let theme = Theme::default();
    let mut md = MarkdownRenderer::new();

    let t1 = md.render_token("| Name ", &theme);
    let t2 = md.render_token("| Role |\n", &theme);
    let t3 = md.render_token("|---|---|\n", &theme);
    assert!(t1.is_empty() && t2.is_empty() && t3.is_empty());

    let t4 = md.render_token("| Alice | Engineer |\n\n", &theme);
    assert_table_contains(&t4, &["Alice", "Engineer"]);
}

#[test]
fn test_pipe_text_without_divider_falls_back_to_text() {
    let theme = Theme::default();
    let mut md = MarkdownRenderer::new();

    let text = "| Just a line with pipes | not a real table\n\n";
    let out = md.render_token(text, &theme);
    assert!(out.contains("Just a line with pipes"));
    assert!(!out.contains('┌') && !out.contains('╭'));
}

#[test]
fn table_cell_wraps_on_word_boundaries() {
    let theme = Theme::default();
    let lines = vec![
        "| Item | Notes |".to_string(),
        "| --- | --- |".to_string(),
        "| Rust | fast reliable memory safe |".to_string(),
    ];
    let rendered = render_markdown_table_at_width(&lines, &theme, 28);
    assert!(rendered.contains("reliable"));
    assert!(rendered.contains("memory"));
    assert!(rendered.contains("safe"));
    assert!(!rendered.contains("reli-"));
}
