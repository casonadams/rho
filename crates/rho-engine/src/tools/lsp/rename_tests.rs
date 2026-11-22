use super::rename::confine;
use super::*;
use crate::lsp::test_server::{FakeOptions, fake_manager};
use crate::tools::undo::UndoTool;
use rho_harness_core::args::UndoArgs;
use std::collections::BTreeMap;
use tempfile::TempDir;

fn project() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Cargo.toml"), "").unwrap();
    std::fs::create_dir(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/lib.rs"), "fn helper() {}\n").unwrap();
    std::fs::write(dir.path().join("src/other.rs"), "fn caller() {\n    helper();\n}\n").unwrap();
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
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("dep.rs"), "").unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(outside.path().join("dep.rs"), dir.path().join("src/link.rs")).unwrap();
        let by_file = BTreeMap::from([(dir.path().join("src/link.rs"), vec![json!({})])]);
        let error = confine(by_file, dir.path()).unwrap_err();
        assert!(error.contains("outside the workspace; nothing was written"), "{error}");
    }
    let inside = BTreeMap::from([(dir.path().join("src/../src/lib.rs"), vec![json!(1)])]);
    let kept = confine(inside, dir.path()).unwrap();
    assert_eq!(kept.len(), 1);
}

#[test]
fn protected_paths_are_refused() {
    let dir = project();
    for name in [".env.rs", ".git/hooks.rs", ".rho/notes.rs"] {
        let by_file = BTreeMap::from([(dir.path().join(name), vec![json!({})])]);
        let error = confine(by_file, dir.path()).unwrap_err();
        assert!(error.contains("a protected path"), "{name}: {error}");
    }
}

#[tokio::test]
async fn rename_is_recorded_per_file_and_undo_reverts_it() {
    let dir = project();
    let (manager, launcher) = fake_manager(dir.path(), FakeOptions::default());
    let history = EditHistory::default();
    let tool = LspTool::new(dir.path(), manager).with_history(Some(history.clone()));
    tool.run(hover("src/other.rs", 2)).await.unwrap();

    let result = tool.run(rename_args("src/lib.rs", 1)).await.unwrap();
    assert!(!result.is_error, "{}", result.content);
    tool.run(hover("src/lib.rs", 1)).await.unwrap();
    assert!(
        launcher
            .received
            .lock()
            .unwrap()
            .iter()
            .any(|method| method == "textDocument/didSave")
    );

    let undo = UndoTool::new(dir.path(), history);
    for path in ["src/lib.rs", "src/other.rs"] {
        let undone = undo.run(UndoArgs { path: path.into() }).await.unwrap();
        assert!(!undone.is_error, "{}", undone.content);
    }
    let lib = std::fs::read_to_string(dir.path().join("src/lib.rs")).unwrap();
    let other = std::fs::read_to_string(dir.path().join("src/other.rs")).unwrap();
    assert_eq!(lib, "fn helper() {}\n");
    assert_eq!(other, "fn caller() {\n    helper();\n}\n");
}

#[cfg(unix)]
#[tokio::test]
async fn a_failed_write_restores_the_files_already_written() {
    use std::os::unix::fs::PermissionsExt;
    let dir = project();
    std::fs::create_dir(dir.path().join("sub")).unwrap();
    std::fs::write(dir.path().join("sub/last.rs"), "fn z() {\n    helper();\n}\n").unwrap();
    let (manager, _) = fake_manager(dir.path(), FakeOptions::default());
    let tool = LspTool::new(dir.path(), manager);
    tool.run(hover("sub/last.rs", 2)).await.unwrap();
    tool.run(hover("src/other.rs", 2)).await.unwrap();

    let locked = dir.path().join("sub");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();
    let enforced = std::fs::write(locked.join("probe"), "").is_err();
    let result = tool.run(rename_args("src/lib.rs", 1)).await.unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    if !enforced {
        return;
    }

    assert!(result.is_error);
    assert!(result.content.contains("rolled back 2 files"), "{}", result.content);
    let lib = std::fs::read_to_string(dir.path().join("src/lib.rs")).unwrap();
    let other = std::fs::read_to_string(dir.path().join("src/other.rs")).unwrap();
    assert_eq!(lib, "fn helper() {}\n");
    assert_eq!(other, "fn caller() {\n    helper();\n}\n");
}

#[tokio::test]
async fn status_omits_the_missing_list_when_every_server_is_installed() {
    let dir = project();
    let (manager, _) = fake_manager(dir.path(), FakeOptions::default());
    let tool = LspTool::new(dir.path(), manager);
    let status = tool
        .run(LspArgs {
            action: LspAction::Status,
            path: None,
            line: None,
            symbol: None,
            query: None,
            new_name: None,
        })
        .await
        .unwrap();
    assert!(status.content.contains("Installed: fake (.rs)"));
    assert!(!status.content.contains("Not on PATH"), "{}", status.content);
}

#[tokio::test]
async fn every_query_sees_files_edited_on_disk_after_they_were_opened() {
    let dir = project();
    let (manager, _) = fake_manager(dir.path(), FakeOptions::default());
    let tool = LspTool::new(dir.path(), manager);
    tool.run(hover("src/other.rs", 2)).await.unwrap();
    std::fs::write(dir.path().join("src/other.rs"), "\nfn caller() {\n    helper();\n}\n").unwrap();

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
