use super::*;
use crate::lsp::test_server::{FakeOptions, fake_manager};
use crate::tools::edit::{EditArgs, EditTool};
use crate::tools::read::{ReadArgs, ReadTool};
use crate::tools::write::{WriteArgs, WriteTool};
use tempfile::TempDir;

const LIB: &str = "struct Config {\n    fn load() {\n    }\n}\nfn helper() {\n    load();\n}\n";

fn project() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Cargo.toml"), "").unwrap();
    std::fs::create_dir(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/lib.rs"), LIB).unwrap();
    std::fs::write(dir.path().join("src/other.rs"), "fn caller() {\n    helper();\n}\n").unwrap();
    dir
}

fn args(action: LspAction) -> LspArgs {
    LspArgs {
        action,
        path: Some("src/lib.rs".into()),
        line: None,
        symbol: None,
        query: None,
        new_name: None,
    }
}

fn at(action: LspAction, path: &str, line: usize, symbol: &str) -> LspArgs {
    LspArgs {
        path: Some(path.into()),
        line: Some(line),
        symbol: Some(symbol.into()),
        ..args(action)
    }
}

async fn run(tool: &LspTool, args: LspArgs) -> ToolResult {
    tool.run(args).await.unwrap()
}

#[tokio::test]
async fn navigation_answers_at_a_line_and_symbol() {
    let dir = project();
    let (manager, launcher) = fake_manager(dir.path(), FakeOptions::default());
    let tool = LspTool::new(dir.path(), manager);

    let definition = run(&tool, at(LspAction::Definition, "src/lib.rs", 6, "load")).await;
    assert_eq!(definition.content, "1 definition\n\nsrc/lib.rs\n2:fn load() {");

    run(&tool, at(LspAction::Hover, "src/other.rs", 2, "helper")).await;
    let references = run(&tool, at(LspAction::References, "src/lib.rs", 5, "helper")).await;
    assert_eq!(
        references.content,
        "2 references\n\nsrc/lib.rs\n5:fn helper() {\n\nsrc/other.rs\n2:helper();"
    );

    let hover = run(&tool, at(LspAction::Hover, "src/lib.rs", 6, "load")).await;
    assert_eq!(hover.content, "`load`: hover");
    assert_eq!(
        launcher.launches.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "one server per root"
    );
}

#[tokio::test]
async fn positions_outside_the_file_or_line_are_errors() {
    let dir = project();
    let (manager, _) = fake_manager(dir.path(), FakeOptions::default());
    let tool = LspTool::new(dir.path(), manager);

    let past_end = run(&tool, at(LspAction::Definition, "src/lib.rs", 99, "load")).await;
    assert!(past_end.is_error);
    assert!(
        past_end.content.contains("line 99 is outside the file (1-7)"),
        "{}",
        past_end.content
    );

    let missing = run(&tool, at(LspAction::Definition, "src/lib.rs", 6, "nope")).await;
    assert!(missing.content.contains("'nope' does not appear on that line: load();"));

    let no_line = run(&tool, args(LspAction::References)).await;
    assert!(no_line.content.contains("needs line"));
}

#[tokio::test]
async fn files_without_a_server_explain_why() {
    let dir = project();
    std::fs::write(dir.path().join("notes.md"), "# notes\n").unwrap();
    let (manager, _) = fake_manager(dir.path(), FakeOptions::default());
    let tool = LspTool::new(dir.path(), manager);
    let result = run(&tool, at(LspAction::Definition, "notes.md", 1, "notes")).await;
    assert!(result.is_error);
    assert!(
        result
            .content
            .contains("no language server is configured for '.md' files")
    );
}

#[tokio::test]
async fn symbols_outline_a_file_or_search_the_workspace() {
    let dir = project();
    let (manager, _) = fake_manager(dir.path(), FakeOptions::default());
    let tool = LspTool::new(dir.path(), manager);

    let outline = run(&tool, args(LspAction::Symbols)).await;
    assert_eq!(
        outline.content,
        "1-4 struct Config\n2-3   function load  fn()\n5-7 function helper  fn()"
    );

    let search = LspArgs {
        path: None,
        query: Some("helper".into()),
        ..args(LspAction::Symbols)
    };
    let found = run(&tool, search).await;
    assert_eq!(found.content, "1 symbol\n\nsrc/lib.rs:5 function helper");
}

#[tokio::test]
async fn rename_rewrites_every_file_the_server_names() {
    let dir = project();
    let (manager, _) = fake_manager(dir.path(), FakeOptions::default());
    let tool = LspTool::new(dir.path(), manager);
    run(&tool, at(LspAction::Hover, "src/other.rs", 2, "helper")).await;

    let rename = LspArgs {
        new_name: Some("assist".into()),
        ..at(LspAction::Rename, "src/lib.rs", 5, "helper")
    };
    let result = run(&tool, rename).await;

    assert_eq!(
        result.content,
        "Renamed to assist in 2 files:\nsrc/lib.rs (1)\nsrc/other.rs (1)"
    );
    let other = std::fs::read_to_string(dir.path().join("src/other.rs")).unwrap();
    assert_eq!(other, "fn caller() {\n    assist();\n}\n");
    let definition = run(&tool, at(LspAction::Definition, "src/other.rs", 2, "assist")).await;
    assert!(definition.content.contains("5:fn assist() {"), "{}", definition.content);
}

#[tokio::test(start_paused = true)]
async fn diagnostics_cover_push_unversioned_and_pull_servers() {
    for options in [
        FakeOptions::default(),
        FakeOptions {
            unversioned: true,
            ..FakeOptions::default()
        },
        FakeOptions {
            pull: true,
            ..FakeOptions::default()
        },
    ] {
        let dir = project();
        std::fs::write(dir.path().join("src/bad.rs"), "fn a() {}\nlet x = ERR;\n// WARN\n").unwrap();
        let (manager, _) = fake_manager(dir.path(), options);
        let tool = LspTool::new(dir.path(), manager);

        let file = run(
            &tool,
            LspArgs {
                path: Some("src/bad.rs".into()),
                ..args(LspAction::Diagnostics)
            },
        )
        .await;
        assert_eq!(
            file.content,
            "LSP (1 error, 1 warning):\n2:9 error [fake] ERR found\n3:4 warning [fake] WARN found"
        );
    }
}

#[tokio::test]
async fn workspace_diagnostics_list_what_running_servers_reported() {
    let dir = project();
    std::fs::write(dir.path().join("src/bad.rs"), "ERR\n").unwrap();
    let (manager, _) = fake_manager(dir.path(), FakeOptions::default());
    let tool = LspTool::new(dir.path(), manager);
    let none = run(
        &tool,
        LspArgs {
            path: None,
            ..args(LspAction::Diagnostics)
        },
    )
    .await;
    assert!(none.content.contains("no language server is running yet"));

    run(
        &tool,
        LspArgs {
            path: Some("src/bad.rs".into()),
            ..args(LspAction::Diagnostics)
        },
    )
    .await;
    let all = run(
        &tool,
        LspArgs {
            path: None,
            ..args(LspAction::Diagnostics)
        },
    )
    .await;
    assert_eq!(
        all.content,
        "1 error, 0 warnings\n\nsrc/bad.rs\n1:1 error [fake] ERR found"
    );
}

#[tokio::test(start_paused = true)]
async fn silent_servers_report_that_diagnostics_are_not_ready() {
    let dir = project();
    let options = FakeOptions {
        silent: true,
        ..FakeOptions::default()
    };
    let (manager, _) = fake_manager(dir.path(), options);
    let report = crate::lsp::diagnostics_for(&manager, &dir.path().join("src/lib.rs"), LIB, Duration::from_millis(50))
        .await
        .unwrap();
    assert_eq!(
        report,
        "LSP: no diagnostics yet\n[fake has not reported diagnostics yet; run lsp diagnostics path=src/lib.rs in a moment]"
    );
}

#[tokio::test(start_paused = true)]
async fn busy_servers_say_so_instead_of_reporting_clean_or_empty_results() {
    let dir = project();
    let options = FakeOptions {
        indexing: true,
        ..FakeOptions::default()
    };
    let (manager, _) = fake_manager(dir.path(), options);
    let tool = LspTool::new(dir.path(), Arc::clone(&manager));

    let missing = run(&tool, at(LspAction::Definition, "src/lib.rs", 1, "struct")).await;
    assert_eq!(
        missing.content,
        "No definition found\n[fake was still busy (Indexing); retry shortly]"
    );
    let found = run(&tool, at(LspAction::Definition, "src/lib.rs", 6, "load")).await;
    assert!(!found.content.contains("busy"), "{}", found.content);

    let report = crate::lsp::diagnostics_for(&manager, &dir.path().join("src/lib.rs"), LIB, Duration::from_secs(3))
        .await
        .unwrap();
    assert_eq!(
        report,
        "LSP so far: no errors or warnings\n[fake is still busy (Indexing); run lsp diagnostics path=src/lib.rs in a moment for the rest]"
    );
}

#[tokio::test]
async fn a_server_that_fails_to_start_is_reported_once_and_not_relaunched() {
    let dir = project();
    let options = FakeOptions {
        fail: true,
        ..FakeOptions::default()
    };
    let (manager, launcher) = fake_manager(dir.path(), options);
    let path = dir.path().join("src/lib.rs");
    let first = crate::lsp::diagnostics_for(&manager, &path, LIB, Duration::from_secs(1)).await;
    assert_eq!(
        first.as_deref(),
        Some("LSP: no diagnostics yet\n[fake: command not found]")
    );
    let second = crate::lsp::diagnostics_for(&manager, &path, LIB, Duration::from_secs(1)).await;
    assert_eq!(second, None);
    assert_eq!(launcher.launches.load(std::sync::atomic::Ordering::SeqCst), 1);

    let tool = LspTool::new(dir.path(), manager);
    let status = run(&tool, args(LspAction::Status)).await;
    assert!(
        status.content.contains("): failed: fake: command not found"),
        "{}",
        status.content
    );
    let diagnostics = run(&tool, args(LspAction::Diagnostics)).await;
    assert_eq!(diagnostics.content, "fake: command not found");
}

#[tokio::test(start_paused = true)]
async fn a_server_that_exits_is_reported_and_restarted_on_next_use() {
    let dir = project();
    let options = FakeOptions {
        exit_on_change: true,
        ..FakeOptions::default()
    };
    let (manager, launcher) = fake_manager(dir.path(), options);
    let path = dir.path().join("src/lib.rs");
    let first = crate::lsp::diagnostics_for(&manager, &path, LIB, Duration::from_secs(3)).await;
    assert_eq!(first.as_deref(), Some("LSP: no errors or warnings"));

    let changed = format!("{LIB}// edit\n");
    let crashed = crate::lsp::diagnostics_for(&manager, &path, &changed, Duration::from_secs(3)).await;
    assert_eq!(crashed.as_deref(), Some("LSP: no diagnostics yet\n[fake exited]"));

    let restarted = crate::lsp::diagnostics_for(&manager, &path, LIB, Duration::from_secs(3)).await;
    assert_eq!(restarted.as_deref(), Some("LSP: no errors or warnings"));
    assert_eq!(launcher.launches.load(std::sync::atomic::Ordering::SeqCst), 2);
}

#[tokio::test(start_paused = true)]
async fn a_failed_start_is_retried_after_a_minute() {
    let dir = project();
    let options = FakeOptions {
        fail: true,
        ..FakeOptions::default()
    };
    let (manager, launcher) = fake_manager(dir.path(), options);
    let path = dir.path().join("src/lib.rs");
    assert!(
        crate::lsp::diagnostics_for(&manager, &path, LIB, Duration::from_secs(1))
            .await
            .is_some()
    );
    tokio::time::advance(crate::lsp::manager::RETRY_FAILED_AFTER + Duration::from_secs(1)).await;
    let again = crate::lsp::diagnostics_for(&manager, &path, LIB, Duration::from_secs(1)).await;
    assert_eq!(
        again.as_deref(),
        Some("LSP: no diagnostics yet\n[fake: command not found]")
    );
    assert_eq!(launcher.launches.load(std::sync::atomic::Ordering::SeqCst), 2);
}

#[tokio::test]
async fn files_outside_the_workspace_use_the_running_server_and_rename_stays_inside() {
    let dir = project();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("dep.rs"), "fn helper() {}\n").unwrap();
    let dep = outside.path().join("dep.rs");
    let (manager, launcher) = fake_manager(dir.path(), FakeOptions::default());
    let tool = LspTool::new(dir.path(), manager);

    let cold = run(&tool, at(LspAction::Hover, dep.to_str().unwrap(), 1, "helper")).await;
    assert!(
        cold.content.contains("outside the workspace and no server is running"),
        "{}",
        cold.content
    );

    run(&tool, at(LspAction::Hover, "src/lib.rs", 5, "helper")).await;
    let warm = run(&tool, at(LspAction::Hover, dep.to_str().unwrap(), 1, "helper")).await;
    assert_eq!(warm.content, "`helper`: hover");
    assert_eq!(launcher.launches.load(std::sync::atomic::Ordering::SeqCst), 1);

    let rename = LspArgs {
        new_name: Some("assist".into()),
        ..at(LspAction::Rename, "src/lib.rs", 5, "helper")
    };
    let refused = run(&tool, rename).await;
    assert!(refused.is_error);
    assert!(
        refused.content.contains("outside the workspace; nothing was written"),
        "{}",
        refused.content
    );
    assert_eq!(std::fs::read_to_string(dir.path().join("src/lib.rs")).unwrap(), LIB);
}

#[tokio::test]
async fn rename_uses_disk_contents_even_when_files_changed_after_opening() {
    let dir = project();
    let (manager, _) = fake_manager(dir.path(), FakeOptions::default());
    let tool = LspTool::new(dir.path(), manager);
    run(&tool, at(LspAction::Hover, "src/other.rs", 2, "helper")).await;
    std::fs::write(
        dir.path().join("src/other.rs"),
        "// moved\nfn caller() {\n    helper();\n}\n",
    )
    .unwrap();

    let rename = LspArgs {
        new_name: Some("assist".into()),
        ..at(LspAction::Rename, "src/lib.rs", 5, "helper")
    };
    run(&tool, rename).await;
    let other = std::fs::read_to_string(dir.path().join("src/other.rs")).unwrap();
    assert_eq!(other, "// moved\nfn caller() {\n    assist();\n}\n");
}

#[tokio::test]
async fn files_outside_the_workspace_get_no_diagnostics_report() {
    let dir = project();
    let outside = tempfile::tempdir().unwrap();
    let (manager, launcher) = fake_manager(dir.path(), FakeOptions::default());
    let path = outside.path().join("x.rs");
    assert_eq!(
        crate::lsp::diagnostics_for(&manager, &path, "ERR", Duration::from_secs(1)).await,
        None
    );
    assert_eq!(launcher.launches.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[tokio::test]
async fn status_lists_started_and_installed_servers() {
    let dir = project();
    let (manager, _) = fake_manager(dir.path(), FakeOptions::default());
    let tool = LspTool::new(dir.path(), manager);
    let before = run(&tool, args(LspAction::Status)).await;
    assert!(before.content.starts_with("No language server started yet"));
    assert!(before.content.contains("Installed: fake (.rs)"));

    run(&tool, args(LspAction::Symbols)).await;
    let after = run(&tool, args(LspAction::Status)).await;
    assert!(after.content.starts_with("Started:\nfake ("), "{}", after.content);
    assert!(after.content.contains("): running"));
}

#[tokio::test]
async fn edits_and_writes_report_the_new_contents_diagnostics() {
    let dir = project();
    let (manager, _) = fake_manager(dir.path(), FakeOptions::default());
    let edit: EditArgs = serde_json::from_value(serde_json::json!({
        "path": "src/lib.rs",
        "edits": [{ "oldText": "    load();", "newText": "    load(ERR);" }]
    }))
    .unwrap();
    let edited = EditTool::new(dir.path())
        .with_lsp(Some(Arc::clone(&manager)))
        .execute(edit)
        .await
        .unwrap();
    assert!(
        edited
            .content
            .ends_with("\n\nLSP (1 error, 0 warnings):\n6:10 error [fake] ERR found"),
        "{}",
        edited.content
    );

    let written = WriteTool::new(dir.path())
        .with_lsp(Some(Arc::clone(&manager)))
        .execute(WriteArgs {
            path: "src/lib.rs".into(),
            content: LIB.into(),
        })
        .await
        .unwrap();
    assert!(
        written.content.ends_with("\n\nLSP: no errors or warnings"),
        "{}",
        written.content
    );

    let plain = WriteTool::new(dir.path())
        .with_lsp(Some(manager))
        .execute(WriteArgs {
            path: "notes.md".into(),
            content: "ERR\n".into(),
        })
        .await
        .unwrap();
    assert!(!plain.content.contains("LSP"), "files without a server get no report");
}

fn read_symbol(symbol: &str) -> ReadArgs {
    ReadArgs {
        path: "src/lib.rs".into(),
        offset: None,
        limit: None,
        symbol: Some(symbol.into()),
    }
}

#[tokio::test]
async fn read_returns_just_the_named_symbol() {
    let dir = project();
    let (manager, _) = fake_manager(dir.path(), FakeOptions::default());
    let read = ReadTool::new(dir.path()).with_lsp(Some(manager));

    let method = read.execute(read_symbol("Config.load")).await.unwrap();
    assert!(
        method.content.starts_with("  2\t    fn load() {\n  3\t    }"),
        "{}",
        method.content
    );
    assert!(
        method
            .content
            .contains("[4 more lines in file. Use offset=4 to continue.]")
    );

    let missing = read.execute(read_symbol("absent")).await.unwrap();
    assert!(missing.is_error);
    assert!(missing.content.starts_with("No symbol 'absent' in"));

    let without = ReadTool::new(dir.path()).execute(read_symbol("load")).await.unwrap();
    assert!(without.content.contains("needs a language server"));
}
