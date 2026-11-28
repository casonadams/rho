use super::*;
use crate::tools::types::ToolResult;
use std::path::Path;

#[test]
fn schema_lists_path_before_edits_so_the_card_can_number_lines_while_edits_stream() {
    let schema = crate::tools::types::generated_schema::<EditArgs>();
    let keys: Vec<&String> = schema["properties"].as_object().unwrap().keys().collect();
    assert_eq!(keys, ["path", "edits"]);
}

fn fixture(label: &str) -> crate::test_support::TestDir {
    crate::test_support::TestDir::with_prefix(&format!("edit-{label}-"))
}

async fn edit_file(workspace: &Path, file: &Path, old_text: &str, new_text: &str) -> ToolResult {
    EditTool::new(workspace)
        .execute(EditArgs {
            path: file.to_str().unwrap().to_string(),
            edits: vec![EditReplacement {
                old_text: old_text.to_string(),
                new_text: new_text.to_string(),
                ..Default::default()
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
}

const SUCCESS_CASES: &[SuccessCase] = &[
    SuccessCase {
        initial: "fn hello() {\n    println!(\"world\");\n}\n",
        old_text: "println!(\"world\");",
        new_text: "println!(\"rust\");",
        expected: "fn hello() {\n    println!(\"rust\");\n}\n",
    },
    SuccessCase {
        initial: "line 1\r\nline 2\r\nline 3\r\n",
        old_text: "line 2\n",
        new_text: "line 2 modified\n",
        expected: "line 1\r\nline 2 modified\r\nline 3\r\n",
    },
];

const EXTERNAL_CASE: SuccessCase = SuccessCase {
    initial: "initial external line\n",
    old_text: "initial external line",
    new_text: "modified external line",
    expected: "modified external line\n",
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
    assert_eq!(read_disk(file).await, case.expected);
}

#[tokio::test]
async fn test_edit_applies_replacements() {
    for case in SUCCESS_CASES {
        let dir = fixture("success");
        assert_edit_applies(&dir, &dir.join("sample.txt"), case).await;
    }
}

#[tokio::test]
async fn test_edit_failures_leave_file_unchanged() {
    for case in FAILURE_CASES {
        let dir = fixture("failure");
        let file = dir.join("sample.txt");
        tokio::fs::write(&file, case.initial).await.unwrap();

        let res = edit_file(&dir, &file, case.old_text, "replacement").await;

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
async fn region_and_replace_all_edits_apply_in_order() {
    let dir = fixture("forms");
    let file = dir.join("lib.rs");
    tokio::fs::write(&file, "use a;\nfn old() {\n    a();\n}\nfn keep() { a(); }\n")
        .await
        .unwrap();
    let args: EditArgs = serde_json::from_value(serde_json::json!({
        "path": file.to_str().unwrap(),
        "edits": [
            { "startText": "fn old() {", "endText": "}\n", "newText": "" },
            { "oldText": "a", "newText": "b", "replaceAll": true }
        ]
    }))
    .unwrap();

    let res = EditTool::new(&dir).execute(args).await.unwrap();

    assert!(!res.is_error, "{}", res.content);
    assert_eq!(read_disk(&file).await, "use b;\nfn keep() { b(); }\n");
}

#[tokio::test]
async fn result_shows_each_changed_region_with_line_numbers() {
    let dir = fixture("preview");
    let file = dir.join("lib.rs");
    let body: String = (1..=20).map(|n| format!("line {n}\n")).collect();
    tokio::fs::write(&file, &body).await.unwrap();
    let args: EditArgs = serde_json::from_value(serde_json::json!({
        "path": file.to_str().unwrap(),
        "edits": [
            { "oldText": "line 3\n", "newText": "three\nthree b\n" },
            { "oldText": "line 15\n", "newText": "" }
        ]
    }))
    .unwrap();

    let res = EditTool::new(&dir).execute(args).await.unwrap();

    let path = file.to_str().unwrap();
    assert_eq!(
        res.content,
        format!(
            "Successfully applied 2 replacement(s) to {path}\n@@ 2-5\n  2\tline 2\n  3\tthree\n  4\tthree b\n  5\tline 4\n@@ 15-16\n 15\tline 14\n 16\tline 16"
        )
    );
}

#[test]
fn preview_caps_the_lines_it_shows() {
    let before: String = (0..200).map(|n| format!("{n}\n")).collect();
    let after: String = (0..200)
        .map(|n| {
            if n % 5 == 0 {
                format!("x{n}\n")
            } else {
                format!("{n}\n")
            }
        })
        .collect();
    let preview = preview::changed_regions(&before, &after);
    let numbered = preview.lines().filter(|line| line.contains('\t')).count();
    assert!(numbered <= preview::PREVIEW_MAX_LINES, "{preview}");
    assert!(
        preview.ends_with("more lines in other changed regions; read the file to see them]"),
        "{preview}"
    );
}

#[tokio::test]
async fn test_edit_rejects_directory() {
    let dir = fixture("dir");
    let sub = dir.join("subdir");
    tokio::fs::create_dir_all(&sub).await.unwrap();

    let res = edit_file(&dir, &sub, "a", "b").await;

    assert!(res.is_error);
    assert!(res.content.contains("target path is a directory"));
}

#[cfg(unix)]
#[tokio::test]
async fn test_edit_inaccessible_file_is_not_reported_as_missing() {
    use std::os::unix::fs::PermissionsExt;
    let dir = fixture("denied");
    let locked = dir.join("locked");
    std::fs::create_dir(&locked).unwrap();
    let file = locked.join("file.txt");
    std::fs::write(&file, "a").unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    let denied = std::fs::metadata(&file).is_err();

    let res = edit_file(&dir, &file, "a", "b").await;
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();

    if !denied {
        return;
    }
    assert!(res.is_error);
    assert!(res.content.starts_with("Failed to access"), "{}", res.content);
    assert!(!res.content.contains("not found"), "{}", res.content);
    assert!(!res.content.contains("Did you mean"), "{}", res.content);
}

#[tokio::test]
async fn test_edit_external_file_successfully() {
    let workspace = fixture("ws");
    let external = fixture("ext");
    assert_edit_applies(&workspace, &external.join("external_edit.txt"), &EXTERNAL_CASE).await;
}

#[tokio::test]
async fn an_lf_file_with_one_stray_crlf_keeps_lf_for_multi_line_edits() {
    let dir = fixture("mixed-lf");
    let file = dir.join("a.txt");
    std::fs::write(&file, "one\ntwo\nthree\nfour\r\nfive\n").unwrap();

    let res = edit_file(&dir, &file, "one\ntwo", "ONE\nTWO\nextra").await;

    assert!(!res.is_error, "{}", res.content);
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "ONE\nTWO\nextra\nthree\nfour\r\nfive\n"
    );
}
