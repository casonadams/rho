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
    assert!(!diff.contains("```diff"));
    assert!(!diff.contains("```"));
    assert!(diff.contains("- let x = 1;"));
    assert!(diff.contains("+ let x = 2;"));
    assert!(diff.contains("+ let y = 3;"));
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
    assert!(!diff.contains("```diff"));
    assert!(!diff.contains("```"));
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
fn test_format_write_preview_renders_syntax_highlighted_code() {
    let theme = Theme::default();
    let args = serde_json::json!({
        "path": "test.py",
        "content": "def main():\n    print('hello')"
    });
    let preview = format_write_preview(&args, &theme, false).unwrap();
    assert!(!preview.contains("```diff"));
    assert!(!preview.contains("```"));
    assert!(!preview.contains("+ def main():"));
    assert!(preview.contains("def"));
    assert!(preview.contains("main"));
    assert!(preview.contains("print"));
    assert!(preview.contains("hello"));
    assert!(preview.contains("  1 │ "));
    assert!(preview.contains("  2 │ "));

    let long_content = (1..=12).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
    let long_args = serde_json::json!({
        "path": "test.txt",
        "content": long_content
    });
    let collapsed = format_write_preview(&long_args, &theme, false).unwrap();
    assert!(collapsed.contains("... (4 more lines, 12 total)"));
    assert!(!collapsed.contains("line 12"));

    let expanded = format_write_preview(&long_args, &theme, true).unwrap();
    assert!(!expanded.contains("more lines"));
    assert!(expanded.contains("line 12"));
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

#[test]
fn test_format_edit_diff_with_explicit_line_number() {
    let theme = Theme::default();
    let args = serde_json::json!({
        "path": "src/main.rs",
        "edits": [
            {
                "oldText": "let a = 1;\nlet b = 2;",
                "newText": "let a = 10;\nlet b = 20;",
                "line": 42
            }
        ]
    });
    let diff = format_edit_diff(&args, &theme).unwrap();
    assert!(diff.contains(" 42 │ "));
    assert!(diff.contains(" 43 │ "));
    assert!(diff.contains("- let a = 1;"));
    assert!(diff.contains("+ let a = 10;"));
}

#[test]
fn test_format_edit_diff_locates_line_from_file_on_disk() {
    let theme = Theme::default();
    let temp_dir = tempfile::tempdir().unwrap();
    let file_path = temp_dir.path().join("example.rs");
    std::fs::write(&file_path, "line 1\nline 2\nline 3\ntarget line\nline 5\n").unwrap();

    let path_str = file_path.to_str().unwrap();
    let args = serde_json::json!({
        "path": path_str,
        "edits": [
            {
                "oldText": "target line",
                "newText": "replaced line"
            }
        ]
    });

    // Before edit on disk (matches oldText at line 4)
    let diff_before = format_edit_diff(&args, &theme).unwrap();
    assert!(diff_before.contains("  4 │ "));
    assert!(diff_before.contains("target"));
    assert!(diff_before.contains("replaced"));

    // After edit on disk (matches newText at line 4)
    std::fs::write(&file_path, "line 1\nline 2\nline 3\nreplaced line\nline 5\n").unwrap();
    let diff_after = format_edit_diff(&args, &theme).unwrap();
    assert!(diff_after.contains("  4 │ "));
    assert!(diff_after.contains("target"));
    assert!(diff_after.contains("replaced"));
}
