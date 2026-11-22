use super::*;
use crate::lsp::test_server::{FakeLauncher, FakeOptions, fake_manager};
use crate::tools::undo::UndoTool;
use rho_harness_core::args::UndoArgs;
use tempfile::TempDir;

fn project(source: &str) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Cargo.toml"), "").unwrap();
    std::fs::create_dir(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/lib.rs"), source).unwrap();
    dir
}

fn request(action: LspAction, line: Option<usize>, query: Option<&str>, apply: bool) -> LspArgs {
    LspArgs {
        action,
        path: Some("src/lib.rs".into()),
        line,
        symbol: None,
        query: query.map(str::to_string),
        new_name: None,
        apply: apply.then_some(true),
    }
}

fn apply(line: usize, query: &str) -> LspArgs {
    request(LspAction::CodeActions, Some(line), Some(query), true)
}

fn lib(dir: &TempDir) -> String {
    std::fs::read_to_string(dir.path().join("src/lib.rs")).unwrap()
}

fn sent(launcher: &FakeLauncher, method: &str) -> Vec<Value> {
    let params = launcher.params.lock().unwrap();
    params
        .iter()
        .filter(|(name, _)| name == method)
        .map(|(_, params)| params.clone())
        .collect()
}

async fn tool_with_history(dir: &TempDir, options: FakeOptions) -> (LspTool, EditHistory, Arc<FakeLauncher>) {
    let (manager, launcher) = fake_manager(dir.path(), options);
    let history = EditHistory::default();
    let tool = LspTool::new(dir.path(), manager).with_history(Some(history.clone()));
    (tool, history, launcher)
}

#[tokio::test(start_paused = true)]
async fn code_actions_list_what_the_server_offers_with_the_lines_diagnostics_as_context() {
    let dir = project("let a = ERR;\nlet b = WARN; CMD\nlet c = 1;\n");
    let (tool, _, launcher) = tool_with_history(&dir, FakeOptions::default()).await;
    tool.run(request(LspAction::Diagnostics, None, None, false))
        .await
        .unwrap();

    let listed = tool
        .run(request(LspAction::CodeActions, Some(2), None, false))
        .await
        .unwrap();

    assert!(!listed.is_error, "{}", listed.content);
    assert_eq!(
        listed.content,
        "2 code actions:\n1. [quickfix] Fix WARN lazily\n2. [refactor] Run fake command (runs a server command; rho cannot apply it)\n[apply one with apply=true and query set to its number or part of its title]"
    );
    let asked = sent(&launcher, "textDocument/codeAction");
    let context = asked[0]["context"]["diagnostics"].as_array().unwrap();
    assert_eq!(context.len(), 1, "only the diagnostics on line 2: {context:?}");
    assert_eq!(context[0]["range"]["start"]["line"], 1);
    assert_eq!(asked[0]["range"]["start"]["line"], 1);

    let nothing = tool
        .run(request(LspAction::CodeActions, Some(3), None, false))
        .await
        .unwrap();
    assert!(
        nothing.content.starts_with("No code actions available there"),
        "{}",
        nothing.content
    );
}

#[tokio::test(start_paused = true)]
async fn applying_by_number_or_title_edits_the_file_records_history_and_undo_reverts_it() {
    let source = "let a = ERR;\nlet b = WARN;\n";
    let dir = project(source);
    let (tool, history, launcher) = tool_with_history(&dir, FakeOptions::default()).await;

    let by_number = tool.run(apply(1, "1")).await.unwrap();
    assert!(!by_number.is_error, "{}", by_number.content);
    assert_eq!(by_number.content, "Applied 'Remove ERR' in 1 file:\nsrc/lib.rs (1)");
    assert_eq!(lib(&dir), "let a = ;\nlet b = WARN;\n");

    let by_title = tool.run(apply(2, "warn lazily")).await.unwrap();
    assert!(!by_title.is_error, "{}", by_title.content);
    assert_eq!(lib(&dir), "let a = ;\nlet b = ok;\n");
    tool.run(request(LspAction::Symbols, None, None, false)).await.unwrap();
    assert!(sent(&launcher, "workspace/didChangeWatchedFiles").len() >= 2);
    assert!(
        launcher
            .received
            .lock()
            .unwrap()
            .iter()
            .any(|m| m == "textDocument/didSave")
    );

    let undo = UndoTool::new(dir.path(), history);
    for expected in ["let a = ;\nlet b = WARN;\n", source] {
        let undone = undo
            .run(UndoArgs {
                path: "src/lib.rs".into(),
            })
            .await
            .unwrap();
        assert!(!undone.is_error, "{}", undone.content);
        assert_eq!(lib(&dir), expected);
    }
}

#[tokio::test(start_paused = true)]
async fn actions_that_cannot_be_applied_say_why_and_change_nothing() {
    let source = "ERR CMD\nSECRET\n";
    let dir = project(source);
    let (tool, _, _) = tool_with_history(&dir, FakeOptions::default()).await;

    let command = tool.run(apply(1, "fake command")).await.unwrap();
    assert!(command.is_error);
    assert!(
        command.content.contains("only runs the server command fake.run"),
        "{}",
        command.content
    );

    let protected = tool.run(apply(2, "secrets")).await.unwrap();
    assert!(protected.is_error);
    assert!(protected.content.contains("a protected path"), "{}", protected.content);
    assert!(
        protected.content.contains("code action would change"),
        "{}",
        protected.content
    );
    assert!(!dir.path().join("src/.env.rs").exists());

    let unnamed = tool
        .run(request(LspAction::CodeActions, Some(1), None, true))
        .await
        .unwrap();
    assert!(unnamed.content.contains("needs query"), "{}", unnamed.content);
    let missing = tool.run(apply(1, "9")).await.unwrap();
    assert!(
        missing.content.contains("no action number 9; the list has 2"),
        "{}",
        missing.content
    );
    let unknown = tool.run(apply(1, "nothing like this")).await.unwrap();
    assert!(
        unknown.content.contains("no action title contains"),
        "{}",
        unknown.content
    );
    let vague = tool.run(apply(1, "r")).await.unwrap();
    assert!(vague.content.contains("matches several actions"), "{}", vague.content);
    assert_eq!(lib(&dir), source);
}

#[tokio::test(start_paused = true)]
async fn format_applies_the_servers_edits_with_the_editorconfig_indent_and_reports_the_regions() {
    let dir = project("fn a() {}  \nfn b() {}\nfn c() {} \n");
    std::fs::write(
        dir.path().join(".editorconfig"),
        "root = true\n[*.rs]\nindent_style = tab\nindent_size = 2\n",
    )
    .unwrap();
    let (tool, history, launcher) = tool_with_history(&dir, FakeOptions::default()).await;

    let formatted = tool.run(request(LspAction::Format, None, None, false)).await.unwrap();

    assert!(!formatted.is_error, "{}", formatted.content);
    assert!(
        formatted.content.starts_with("Formatted src/lib.rs\n"),
        "{}",
        formatted.content
    );
    assert!(formatted.content.contains("fn a() {}"), "{}", formatted.content);
    assert_eq!(lib(&dir), "fn a() {}\nfn b() {}\nfn c() {}\n");
    let options = &sent(&launcher, "textDocument/formatting")[0]["options"];
    assert_eq!(options, &json!({ "tabSize": 2, "insertSpaces": false }));

    let again = tool.run(request(LspAction::Format, None, None, false)).await.unwrap();
    assert_eq!(again.content, "src/lib.rs is already formatted");

    let undone = UndoTool::new(dir.path(), history)
        .run(UndoArgs {
            path: "src/lib.rs".into(),
        })
        .await
        .unwrap();
    assert!(!undone.is_error, "{}", undone.content);
    assert_eq!(lib(&dir), "fn a() {}  \nfn b() {}\nfn c() {} \n");
}

#[tokio::test(start_paused = true)]
async fn format_defaults_to_four_spaces_without_an_editorconfig() {
    let dir = project("fn a() {} \n");
    let (tool, _, launcher) = tool_with_history(&dir, FakeOptions::default()).await;

    tool.run(request(LspAction::Format, None, None, false)).await.unwrap();

    let options = &sent(&launcher, "textDocument/formatting")[0]["options"];
    assert_eq!(options, &json!({ "tabSize": 4, "insertSpaces": true }));
}

#[tokio::test(start_paused = true)]
async fn an_empty_answer_from_a_quiet_new_server_suggests_retrying_until_the_window_passes() {
    let dir = project("fn helper() {}\nfn caller() { nothere(); }\n");
    let quiet = FakeOptions {
        no_status: true,
        ..FakeOptions::default()
    };
    let (tool, _, _) = tool_with_history(&dir, quiet).await;
    let query = || LspArgs {
        symbol: Some("nothere".into()),
        ..request(LspAction::Definition, Some(2), None, false)
    };

    let early = tool.run(query()).await.unwrap();
    assert!(
        early.content.contains("may still be indexing; retry in a few seconds"),
        "{}",
        early.content
    );

    tokio::time::sleep(INDEXING_WINDOW + Duration::from_secs(1)).await;
    let late = tool.run(query()).await.unwrap();
    assert!(!late.content.contains("indexing"), "{}", late.content);
}

#[tokio::test(start_paused = true)]
async fn servers_that_report_status_never_get_the_indexing_hint() {
    let dir = project("fn helper() {}\nfn caller() { nothere(); }\n");
    let (tool, _, _) = tool_with_history(&dir, FakeOptions::default()).await;
    let query = LspArgs {
        symbol: Some("nothere".into()),
        ..request(LspAction::Definition, Some(2), None, false)
    };

    let answer = tool.run(query).await.unwrap();

    assert!(!answer.content.contains("indexing"), "{}", answer.content);
}

fn change_types(launcher: &FakeLauncher) -> Vec<u64> {
    sent(launcher, "workspace/didChangeWatchedFiles")
        .iter()
        .flat_map(|params| params["changes"].as_array().unwrap().clone())
        .map(|change| change["type"].as_u64().unwrap())
        .collect()
}

#[tokio::test(start_paused = true)]
async fn edit_write_undo_and_rename_each_announce_the_files_they_change() {
    use crate::tools::edit::{EditArgs, EditReplacement, EditTool};
    use crate::tools::write::{WriteArgs, WriteTool};
    let dir = project("fn helper() {}\n");
    let (manager, launcher) = fake_manager(dir.path(), FakeOptions::default());
    let history = EditHistory::default();
    manager.client_for(&dir.path().join("src/lib.rs")).await.unwrap();
    let lsp = Some(Arc::clone(&manager));

    let edit = EditTool::new(dir.path())
        .with_lsp(lsp.clone())
        .with_history(Some(history.clone()));
    let replacement = EditReplacement {
        old_text: "helper".into(),
        new_text: "assist".into(),
        ..EditReplacement::default()
    };
    edit.execute(EditArgs {
        path: "src/lib.rs".into(),
        edits: vec![replacement],
    })
    .await
    .unwrap();
    let write = WriteTool::new(dir.path())
        .with_lsp(lsp.clone())
        .with_history(Some(history.clone()));
    let created = WriteArgs {
        path: "src/new.rs".into(),
        content: "fn n() {}\n".into(),
    };
    write.execute(created).await.unwrap();
    let rename = LspTool::new(dir.path(), Arc::clone(&manager)).with_history(Some(history.clone()));
    let renamed = LspArgs {
        symbol: Some("assist".into()),
        new_name: Some("help".into()),
        ..request(LspAction::Rename, Some(1), None, false)
    };
    assert!(!rename.run(renamed).await.unwrap().is_error);
    let undo = UndoTool::new(dir.path(), history).with_lsp(lsp);
    undo.run(UndoArgs {
        path: "src/new.rs".into(),
    })
    .await
    .unwrap();
    manager
        .client_for(&dir.path().join("src/lib.rs"))
        .await
        .unwrap()
        .request("workspace/symbol", json!({ "query": "" }))
        .await
        .unwrap();

    assert_eq!(
        change_types(&launcher),
        [2, 1, 2, 3],
        "edit, write(create), rename, undo(delete)"
    );
}

#[tokio::test(start_paused = true)]
async fn a_tool_bound_to_a_turn_delivers_late_diagnostics_to_the_turns_mailbox() {
    use crate::tools::edit::{EditArgs, EditReplacement, EditTool};
    use crate::tools::engine_tool::{EngineTool, TurnToolContext};
    let dir = project("fn ok() {}\n");
    let options = FakeOptions {
        publish_delay_ms: 5000,
        ..FakeOptions::default()
    };
    let (manager, _) = fake_manager(dir.path(), options);
    let inbox = crate::mailbox::Mailboxes::default().open("turn-agent", None);
    let context = TurnToolContext {
        inbox: Some(inbox.clone()),
        ..TurnToolContext::default()
    };
    let unbound = EditTool::new(dir.path()).with_lsp(Some(manager));
    let bound = unbound
        .bind_turn(&context)
        .expect("a tool with a language server binds to the turn's mailbox");
    let args = serde_json::to_value(EditArgs {
        path: "src/lib.rs".into(),
        edits: vec![EditReplacement {
            old_text: "ok".into(),
            new_text: "ERR".into(),
            ..EditReplacement::default()
        }],
    })
    .unwrap();

    let result = bound.execute(args).await.unwrap();
    assert!(
        result.content.contains("more diagnostics will follow automatically"),
        "{}",
        result.content
    );

    tokio::time::timeout(Duration::from_secs(12), inbox.wait())
        .await
        .unwrap();
    assert!(inbox.take()[0].body.contains("1 error"));
}
