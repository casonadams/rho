use super::super::formatters::{
    format_edit_diff, format_relative_time, format_session_status, format_thinking_block, format_write_preview,
};
use crate::ui::theme::Theme;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use rho_harness_core::presentation::SessionStatus;

fn assert_contains_all(s: &str, items: &[&str]) {
    for item in items {
        assert!(s.contains(item));
    }
}

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
    assert!(!diff.contains("```diff") && !diff.contains("```"));
    assert_contains_all(&diff, &["- let x = 1;", "+ let x = 2;", "+ let y = 3;"]);
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
    assert!(!diff.contains("```diff") && !diff.contains("```"));
    assert_contains_all(
        &diff,
        &[
            "-     let ",
            "+     let ",
            "\x1b[7mold_val\x1b[27m",
            "\x1b[7mnew_val\x1b[27m",
            " = 10;",
        ],
    );
}

#[test]
fn test_format_write_preview_syntax_highlighting() {
    let theme = Theme::default();
    let args = serde_json::json!({
        "path": "test.py",
        "content": "def main():\n    print('hello')"
    });
    let preview = format_write_preview(&args, &theme, false).unwrap();
    assert!(!preview.contains("```diff") && !preview.contains("```") && !preview.contains("+ def main():"));
    assert_contains_all(&preview, &["def", "main", "print", "hello", "  1 │ ", "  2 │ "]);
}

#[test]
fn test_format_write_preview_expansion_and_collapse() {
    let theme = Theme::default();
    let long_content = (1..=12).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
    let long_args = serde_json::json!({
        "path": "test.txt",
        "content": long_content
    });
    let collapsed = format_write_preview(&long_args, &theme, false).unwrap();
    assert!(collapsed.contains("... (4 more lines, 12 total)") && !collapsed.contains("line 12"));

    let expanded = format_write_preview(&long_args, &theme, true).unwrap();
    assert!(!expanded.contains("more lines") && expanded.contains("line 12"));
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

    let diff_before = format_edit_diff(&args, &theme).unwrap();
    assert_contains_all(&diff_before, &["  4 │ ", "target", "replaced"]);

    std::fs::write(&file_path, "line 1\nline 2\nline 3\nreplaced line\nline 5\n").unwrap();
    let diff_after = format_edit_diff(&args, &theme).unwrap();
    assert_contains_all(&diff_after, &["  4 │ ", "target", "replaced"]);
}

#[test]
fn relative_time_buckets_and_boundaries() {
    let now = Utc::now();
    let old = now - ChronoDuration::days(40);
    let cases: [(DateTime<Utc>, String); 15] = [
        (now, "just now".to_string()),
        (now - ChronoDuration::seconds(30), "just now".to_string()),
        (now - ChronoDuration::seconds(59), "just now".to_string()),
        (now - ChronoDuration::seconds(60), "1m ago".to_string()),
        (now - ChronoDuration::minutes(5), "5m ago".to_string()),
        (now - ChronoDuration::minutes(59), "59m ago".to_string()),
        (now - ChronoDuration::minutes(60), "1h ago".to_string()),
        (now - ChronoDuration::hours(3), "3h ago".to_string()),
        (now - ChronoDuration::hours(23), "23h ago".to_string()),
        (now - ChronoDuration::hours(24), "1d ago".to_string()),
        (now - ChronoDuration::days(2), "2d ago".to_string()),
        (now - ChronoDuration::days(4), "4d ago".to_string()),
        (now - ChronoDuration::days(10), "10d ago".to_string()),
        (now - ChronoDuration::days(29), "29d ago".to_string()),
        (
            now - ChronoDuration::days(30),
            (now - ChronoDuration::days(30)).format("%Y-%m-%d").to_string(),
        ),
    ];
    for (time, expected) in cases {
        assert_eq!(format_relative_time(time), expected);
    }
    assert_eq!(format_relative_time(old), old.format("%Y-%m-%d").to_string());
    assert!(format_relative_time(now - ChronoDuration::days(45)).contains('-'));
}

#[test]
fn session_status_joins_model_context_and_optional_quota() {
    let cases = [
        ("claude-sonnet", "27.4% (1M)", None, "claude-sonnet | 27.4% (1M)"),
        (
            "claude-sonnet",
            "27.4% (1M)",
            Some("93% (3h22m)"),
            "claude-sonnet | 27.4% (1M) | 93% (3h22m)",
        ),
        (
            "qwen-日本語モデル",
            "0% (376k)",
            Some("80% quota"),
            "qwen-日本語モデル | 0% (376k) | 80% quota",
        ),
    ];
    for (model, context, quota, expected) in cases {
        let session = SessionStatus {
            model: model.to_string(),
            provider: "test".to_string(),
            context: context.to_string(),
            quota: quota.map(str::to_string),
        };
        assert_eq!(format_session_status(&session), expected);
    }
}
