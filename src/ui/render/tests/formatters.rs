use super::super::formatters::{format_edit_diff, format_thinking_block, format_write_preview};
use crate::ui::theme::Theme;

#[test]
fn test_format_edit_diff_renders_removals_and_additions() {
    let theme = Theme::default();
    let args = serde_json::json!({
        "path": "src/main.rs",
        "edits": [
            {
                "oldText": "let x = 1;",
                "newText": "let x = 2;\nlet y = 3;"
            }
        ]
    });
    let diff = format_edit_diff(&args, &theme).unwrap();
    assert!(diff.contains("```diff"));
    assert!(diff.contains("- let x = 1;"));
    assert!(diff.contains("+ let x = 2;"));
    assert!(diff.contains("+ let y = 3;"));
    assert!(diff.contains("```"));
    assert!(diff.ends_with('\n'));
}

#[test]
fn test_format_edit_diff_intra_line_word_highlighting() {
    let theme = Theme::default();
    let args = serde_json::json!({
        "path": "src/main.rs",
        "edits": [
            {
                "oldText": "    let old_val = 10;",
                "newText": "    let new_val = 10;"
            }
        ]
    });
    let diff = format_edit_diff(&args, &theme).unwrap();
    assert!(diff.contains("```diff"));
    // Verify unchanged leading indentation is preserved without inverse escape
    assert!(diff.contains("-     let "));
    assert!(diff.contains("+     let "));
    // Verify modified tokens are wrapped in inverse video \x1b[7m ... \x1b[27m
    assert!(diff.contains("\x1b[7mold_val\x1b[27m"));
    assert!(diff.contains("\x1b[7mnew_val\x1b[27m"));
    // Verify trailing unchanged token is preserved
    assert!(diff.contains(" = 10;"));
}

#[test]
fn test_format_write_preview_renders_additions() {
    let theme = Theme::default();
    let args = serde_json::json!({
        "path": "test.py",
        "content": "def main():\n    print('hello')"
    });
    let preview = format_write_preview(&args, &theme, false).unwrap();
    assert!(preview.contains("```diff"));
    assert!(preview.contains("+ def main():"));
    assert!(preview.contains("+     print('hello')"));
    assert!(preview.contains("```"));

    let long_content = (1..=12).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
    let long_args = serde_json::json!({
        "path": "test.txt",
        "content": long_content
    });
    let collapsed = format_write_preview(&long_args, &theme, false).unwrap();
    assert!(collapsed.contains("... (4 more lines, 12 total, Ctrl+O to expand)"));
    assert!(!collapsed.contains("+ line 12"));

    let expanded = format_write_preview(&long_args, &theme, true).unwrap();
    assert!(!expanded.contains("Ctrl+O to expand"));
    assert!(expanded.contains("+ line 12"));
}

#[test]
fn test_format_thinking_block_renders_dimmed_with_trailing_breaks() {
    let theme = Theme::default();
    let formatted = format_thinking_block("analyzing the problem\nchecking tests", &theme);
    assert!(formatted.contains("analyzing the problem"));
    assert!(formatted.contains("checking tests"));
    assert!(!formatted.contains("┌─ Thinking"));
    assert!(formatted.ends_with('\n'));
}
