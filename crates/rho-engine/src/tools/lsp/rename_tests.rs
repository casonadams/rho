use super::rename::{Planned, confine};
use super::*;
use crate::lsp::test_server::{FakeOptions, fake_manager};
use crate::test_support::TestDir;
use crate::tools::undo::UndoTool;
use rho_harness_core::args::UndoArgs;
use std::collections::BTreeMap;

fn project() -> TestDir {
    let dir = TestDir::new();
    std::fs::write(dir.join("Cargo.toml"), "").unwrap();
    std::fs::create_dir(dir.join("src")).unwrap();
    std::fs::write(dir.join("src/lib.rs"), "fn helper() {}\n").unwrap();
    std::fs::write(dir.join("src/other.rs"), "fn caller() {\n    helper();\n}\n").unwrap();
    dir
}

fn rename_args(path: &str, line: usize) -> LspArgs {
    LspArgs {
        action: LspAction::Rename,
        path: Some(path.into()),
        line: Some(line),
        symbol: Some("helper".into()),
        query: None,
        new_name: Some("assist".into()),
        apply: None,
    }
}

fn hover(path: &str, line: usize) -> LspArgs {
    LspArgs {
        action: LspAction::Hover,
        ..rename_args(path, line)
    }
}

#[test]
fn symlinks_are_resolved_before_the_workspace_check() {
    let dir = project();
    let outside = TestDir::new();
    std::fs::write(outside.join("dep.rs"), "").unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(outside.join("dep.rs"), dir.join("src/link.rs")).unwrap();
        let by_file = BTreeMap::from([(dir.join("src/link.rs"), vec![json!({})])]);
        let error = confine(by_file, &dir, "rename").unwrap_err();
        assert!(error.contains("outside the workspace; nothing was written"), "{error}");
    }
    let inside = BTreeMap::from([(dir.join("src/../src/lib.rs"), vec![json!(1)])]);
    let kept = confine(inside, &dir, "rename").unwrap();
    assert_eq!(kept.len(), 1);
}

#[test]
fn protected_paths_are_refused() {
    let dir = project();
    for name in [".env.rs", ".git/hooks.rs", ".rho/notes.rs"] {
        let by_file = BTreeMap::from([(dir.join(name), vec![json!({})])]);
        let error = confine(by_file, &dir, "rename").unwrap_err();
        assert!(error.contains("a protected path"), "{name}: {error}");
    }
}

#[tokio::test]
async fn rename_is_recorded_per_file_and_undo_reverts_it() {
    let dir = project();
    let (manager, launcher) = fake_manager(&dir, FakeOptions::default());
    let history = EditHistory::default();
    let tool = LspTool::new(&dir, manager).with_history(Some(history.clone()));
    tool.run(hover("src/other.rs", 2)).await.unwrap();

    let result = tool.run(rename_args("src/lib.rs", 1)).await.unwrap();
    assert!(!result.is_error, "{}", result.content);
    let post_rename_hover = LspArgs {
        symbol: Some("assist".into()),
        ..hover("src/lib.rs", 1)
    };
    let hover_result = tool.run(post_rename_hover).await.unwrap();
    assert!(!hover_result.is_error, "{}", hover_result.content);
    assert!(
        launcher
            .received
            .lock()
            .unwrap()
            .iter()
            .any(|method| method == "textDocument/didSave")
    );

    let undo = UndoTool::new(&dir, history);
    for path in ["src/lib.rs", "src/other.rs"] {
        let undone = undo.run(UndoArgs { path: path.into() }).await.unwrap();
        assert!(!undone.is_error, "{}", undone.content);
    }
    let lib = std::fs::read_to_string(dir.join("src/lib.rs")).unwrap();
    let other = std::fs::read_to_string(dir.join("src/other.rs")).unwrap();
    assert_eq!(lib, "fn helper() {}\n");
    assert_eq!(other, "fn caller() {\n    helper();\n}\n");
}

fn plan(dir: &Path, name: &str, before: &str, next: &str) -> Planned {
    Planned {
        path: std::fs::canonicalize(dir).unwrap().join(name),
        before: before.into(),
        next: next.into(),
        count: 1,
    }
}

fn edit_of(file: &OpenFile, from: usize, to: usize, new_text: &str) -> Value {
    let range = json!({ "start": { "line": 0, "character": from }, "end": { "line": 0, "character": to } });
    json!({ "changes": { file_uri(&file.path): [{ "range": range, "newText": new_text }] } })
}

async fn assert_nothing_to_undo(dir: &Path, history: EditHistory, path: &str) {
    let undone = UndoTool::new(dir, history)
        .run(UndoArgs { path: path.into() })
        .await
        .unwrap();
    assert!(undone.is_error, "{}", undone.content);
    assert!(undone.content.contains("no edit or write"), "{}", undone.content);
}

#[tokio::test(start_paused = true)]
async fn a_failed_write_restores_the_files_already_written() {
    let dir = project();
    let (manager, _) = fake_manager(&dir, FakeOptions::default());
    let tool = LspTool::new(&dir, manager);
    let file = tool.open(Some("src/lib.rs")).await.unwrap();
    let planned = vec![
        plan(&dir, "src/lib.rs", "fn helper() {}\n", "fn assist() {}\n"),
        plan(&dir, "src/other.rs", "fn caller() {\n    helper();\n}\n", "x\n"),
    ];
    let fail_other = async |path: &Path, content: &[u8]| {
        if path.ends_with("other.rs") {
            return Err(std::io::Error::other("disk full"));
        }
        tokio::fs::write(path, content).await
    };

    let error = tool.commit(planned, &file.client, fail_other).await.err().unwrap();

    assert!(error.contains("disk full; rolled back 1 file"), "{error}");
    let lib = std::fs::read_to_string(dir.join("src/lib.rs")).unwrap();
    assert_eq!(lib, "fn helper() {}\n");
}

#[tokio::test(start_paused = true)]
async fn files_left_changed_by_a_failed_restore_are_recorded_and_announced() {
    let dir = project();
    let (manager, _) = fake_manager(&dir, FakeOptions::default());
    let history = EditHistory::default();
    let tool = LspTool::new(&dir, Arc::clone(&manager)).with_history(Some(history.clone()));
    let file = tool.open(Some("src/lib.rs")).await.unwrap();
    let planned = vec![
        plan(&dir, "src/lib.rs", "fn helper() {}\n", "fn assist() {}\n"),
        plan(&dir, "src/other.rs", "fn caller() {\n    helper();\n}\n", "x\n"),
    ];
    let broken = async |path: &Path, content: &[u8]| {
        if path.ends_with("other.rs") || content == b"fn helper() {}\n" {
            return Err(std::io::Error::other("disk gone"));
        }
        tokio::fs::write(path, content).await
    };

    let error = tool.commit(planned, &file.client, broken).await.err().unwrap();

    assert!(error.contains("could not restore"), "{error}");
    let lib = dir.join("src/lib.rs");
    assert_eq!(std::fs::read_to_string(&lib).unwrap(), "fn assist() {}\n");
    assert_eq!(
        manager.change_count(&lib),
        1,
        "servers are told about the file left changed"
    );
    let undone = UndoTool::new(&dir, history)
        .run(UndoArgs {
            path: "src/lib.rs".into(),
        })
        .await
        .unwrap();
    assert!(!undone.is_error, "{}", undone.content);
    assert_eq!(std::fs::read_to_string(&lib).unwrap(), "fn helper() {}\n");
}

#[tokio::test(start_paused = true)]
async fn an_edit_for_text_that_changed_on_disk_since_the_server_read_it_is_refused() {
    let dir = project();
    let (manager, _) = fake_manager(&dir, FakeOptions::default());
    let history = EditHistory::default();
    let tool = LspTool::new(&dir, manager).with_history(Some(history.clone()));
    let file = tool.open(Some("src/lib.rs")).await.unwrap();
    let moved = "// header\nfn helper() {}\n";
    std::fs::write(dir.join("src/lib.rs"), moved).unwrap();

    let refused = tool
        .apply_workspace_edit(&edit_of(&file, 3, 9, "assist"), &file, "rename")
        .await
        .err()
        .unwrap();

    assert!(
        refused.contains("changed on disk") && refused.contains("Retry the rename"),
        "{refused}"
    );
    assert_eq!(std::fs::read_to_string(dir.join("src/lib.rs")).unwrap(), moved);
    assert_nothing_to_undo(&dir, history, "src/lib.rs").await;
}

#[tokio::test(start_paused = true)]
async fn an_edit_that_changes_nothing_writes_nothing_and_records_no_revision() {
    let dir = project();
    let (manager, _) = fake_manager(&dir, FakeOptions::default());
    let history = EditHistory::default();
    let tool = LspTool::new(&dir, Arc::clone(&manager)).with_history(Some(history.clone()));
    let file = tool.open(Some("src/lib.rs")).await.unwrap();

    let applied = tool
        .apply_workspace_edit(&edit_of(&file, 3, 9, "helper"), &file, "rename")
        .await
        .ok()
        .unwrap();

    assert!(applied.files.is_empty());
    assert_eq!(manager.change_count(&dir.join("src/lib.rs")), 0);
    assert_nothing_to_undo(&dir, history, "src/lib.rs").await;
}

#[tokio::test]
async fn status_omits_the_missing_list_when_every_server_is_installed() {
    let dir = project();
    let (manager, _) = fake_manager(&dir, FakeOptions::default());
    let tool = LspTool::new(&dir, manager);
    let status = tool
        .run(LspArgs {
            action: LspAction::Status,
            path: None,
            line: None,
            symbol: None,
            query: None,
            new_name: None,
            apply: None,
        })
        .await
        .unwrap();
    assert!(status.content.contains("Installed: fake (.rs)"));
    assert!(!status.content.contains("Not on PATH"), "{}", status.content);
}

#[tokio::test]
async fn every_query_sees_files_edited_on_disk_after_they_were_opened() {
    let dir = project();
    let (manager, _) = fake_manager(&dir, FakeOptions::default());
    let tool = LspTool::new(&dir, manager);
    tool.run(hover("src/other.rs", 2)).await.unwrap();
    std::fs::write(dir.join("src/other.rs"), "\nfn caller() {\n    helper();\n}\n").unwrap();

    let references = tool
        .run(LspArgs {
            action: LspAction::References,
            ..rename_args("src/lib.rs", 1)
        })
        .await
        .unwrap();
    assert!(
        references.content.contains("src/other.rs\n3:helper();"),
        "{}",
        references.content
    );
}
