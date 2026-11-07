use super::*;
use crate::tools::types::ToolResult;
use std::path::Path;

fn fixture(label: &str) -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(&format!("edit-{label}-"))
        .tempdir()
        .unwrap()
}

async fn edit_file(workspace: &Path, file: &Path, old_text: &str, new_text: &str) -> ToolResult {
    EditTool::new(workspace)
        .execute(EditArgs {
            path: file.to_str().unwrap().to_string(),
            edits: vec![EditReplacement {
                old_text: old_text.to_string(),
                new_text: new_text.to_string(),
            }],
        })
        .await
        .unwrap()
}

async fn read_disk(path: &Path) -> String {
    tokio::fs::read_to_string(path).await.unwrap()
}

struct SuccessCase {
    initial: &'static str,
    old_text: &'static str,
    new_text: &'static str,
    expected: &'static str,
    line: Option<usize>,
}

const SUCCESS_CASES: &[SuccessCase] = &[
    SuccessCase {
        initial: "fn hello() {\n    println!(\"world\");\n}\n",
        old_text: "println!(\"world\");",
        new_text: "println!(\"rust\");",
        expected: "fn hello() {\n    println!(\"rust\");\n}\n",
        line: Some(2),
    },
    SuccessCase {
        initial: "line 1\r\nline 2\r\nline 3\r\n",
        old_text: "line 2\n",
        new_text: "line 2 modified\n",
        expected: "line 1\r\nline 2 modified\r\nline 3\r\n",
        line: None,
    },
];

const EXTERNAL_CASE: SuccessCase = SuccessCase {
    initial: "initial external line\n",
    old_text: "initial external line",
    new_text: "modified external line",
    expected: "modified external line\n",
    line: None,
};

struct FailureCase {
    name: &'static str,
    initial: &'static str,
    old_text: &'static str,
    expected: &'static [&'static str],
    unexpected: &'static [&'static str],
}

const FAILURE_CASES: &[FailureCase] = &[
    FailureCase {
        name: "duplicate match",
        initial: "foo bar foo baz\n",
        old_text: "foo",
        expected: &["found 2 times", "Provide more surrounding context"],
        unexpected: &[],
    },
    FailureCase {
        name: "whitespace hint",
        initial: "    fn calculate() {\n        let x = 1;\n    }\n",
        old_text: "  fn calculate() {\n    let x = 1;\n  }",
        expected: &["oldText not found", "different whitespace or indentation"],
        unexpected: &[],
    },
    FailureCase {
        name: "missing match",
        initial: "hello world\n",
        old_text: "not_present",
        expected: &["oldText not found"],
        unexpected: &["different whitespace or indentation"],
    },
];

async fn assert_edit_applies(workspace: &Path, file: &Path, case: &SuccessCase) {
    tokio::fs::write(file, case.initial).await.unwrap();

    let res = edit_file(workspace, file, case.old_text, case.new_text).await;

    assert!(!res.is_error, "{}", res.content);
    if let Some(line) = case.line {
        assert_eq!(res.metadata, Some(serde_json::json!({ "line_numbers": [line] })));
    }
    assert_eq!(read_disk(file).await, case.expected);
}

#[tokio::test]
async fn test_edit_applies_replacements() {
    for case in SUCCESS_CASES {
        let dir = fixture("success");
        assert_edit_applies(dir.path(), &dir.path().join("sample.txt"), case).await;
    }
}

#[tokio::test]
async fn test_edit_failures_leave_file_unchanged() {
    for case in FAILURE_CASES {
        let dir = fixture("failure");
        let file = dir.path().join("sample.txt");
        tokio::fs::write(&file, case.initial).await.unwrap();

        let res = edit_file(dir.path(), &file, case.old_text, "replacement").await;

        assert!(res.is_error, "{}", case.name);
        for needle in case.expected {
            assert!(res.content.contains(needle), "{}: missing {needle:?}", case.name);
        }
        for needle in case.unexpected {
            assert!(!res.content.contains(needle), "{}: unexpected {needle:?}", case.name);
        }
        assert_eq!(read_disk(&file).await, case.initial, "{}", case.name);
    }
}

#[tokio::test]
async fn test_edit_rejects_directory() {
    let dir = fixture("dir");
    let sub = dir.path().join("subdir");
    tokio::fs::create_dir_all(&sub).await.unwrap();

    let res = edit_file(dir.path(), &sub, "a", "b").await;

    assert!(res.is_error);
    assert!(res.content.contains("target path is a directory"));
}

#[tokio::test]
async fn test_edit_external_file_successfully() {
    let workspace = fixture("ws");
    let external = fixture("ext");
    assert_edit_applies(
        workspace.path(),
        &external.path().join("external_edit.txt"),
        &EXTERNAL_CASE,
    )
    .await;
}
