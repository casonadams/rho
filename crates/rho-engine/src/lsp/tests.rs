use super::*;
use crate::lsp::test_server::{FakeOptions, fake_manager, fake_manager_with, fake_server};
use serde_json::{Value, json};

fn project() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Cargo.toml"), "").unwrap();
    std::fs::create_dir(dir.path().join("src")).unwrap();
    let lib = dir.path().join("src/lib.rs");
    std::fs::write(&lib, "fn ok() {}\n").unwrap();
    (dir, lib)
}

#[tokio::test(start_paused = true)]
async fn errors_in_other_files_count_only_files_that_still_exist() {
    let (dir, lib) = project();
    let other = dir.path().join("src/other.rs");
    std::fs::write(&other, "ERR\n").unwrap();
    let (manager, _) = fake_manager(dir.path(), FakeOptions::default());
    let client = manager.client_for(&lib).await.unwrap();
    client.sync(&other, "ERR\n", "rust").await.unwrap();
    let wait = Duration::from_secs(3);
    while client.state().all_published().is_empty() {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    let report = diagnostics_for(&manager, &lib, "fn ok() {}\n", wait).await.unwrap();
    assert_eq!(
        report,
        "LSP: no errors or warnings\n[1 error in other files; lsp diagnostics lists them]"
    );

    std::fs::remove_file(&other).unwrap();
    let report = diagnostics_for(&manager, &lib, "fn ok() {}\n", wait).await.unwrap();
    assert_eq!(report, "LSP: no errors or warnings");
}

#[tokio::test(start_paused = true)]
async fn every_server_reports_within_one_shared_deadline() {
    let (dir, lib) = project();
    let options = FakeOptions {
        indexing: true,
        ..FakeOptions::default()
    };
    let servers = vec![fake_server("fake", false), fake_server("lint", true)];
    let (manager, _) = fake_manager_with(dir.path(), options, servers);

    let started = Instant::now();
    let report = diagnostics_for(&manager, &lib, "ERR\n", Duration::from_secs(3))
        .await
        .unwrap();
    assert!(started.elapsed() <= Duration::from_secs(3), "{:?}", started.elapsed());
    assert!(report.starts_with("LSP so far (1 error, 0 warnings)"), "{report}");
    for name in ["fake", "lint"] {
        let note =
            format!("{name} is still busy (Indexing); run lsp diagnostics path=src/lib.rs in a moment for the rest");
        assert!(report.contains(&note), "{report}");
    }
}

fn late_server() -> FakeOptions {
    FakeOptions {
        publish_delay_ms: 5000,
        ..FakeOptions::default()
    }
}

async fn edited(manager: &Arc<LspManager>, inbox: Option<&crate::mailbox::Mailbox>, lib: &std::path::Path) -> String {
    let result = crate::tools::types::ToolResult::success("edited");
    append_diagnostics(result, Some(manager), inbox, lib, "ERR\n", false)
        .await
        .content
}

#[tokio::test(start_paused = true)]
async fn diagnostics_that_miss_the_wait_arrive_later_as_an_untrusted_notice() {
    let (dir, lib) = project();
    let (manager, _) = fake_manager(dir.path(), late_server());
    let inbox = crate::mailbox::Mailboxes::default().open("agent", None);

    let content = edited(&manager, Some(&inbox), &lib).await;
    assert!(content.contains("LSP: no diagnostics yet"), "{content}");
    assert!(
        content.contains("more diagnostics will follow automatically if the server reports any"),
        "{content}"
    );
    assert!(!content.contains("run lsp diagnostics"), "{content}");
    assert_eq!(inbox.pending(), 0);

    tokio::time::timeout(Duration::from_secs(12), inbox.wait())
        .await
        .unwrap();
    let notices = inbox.take();
    assert_eq!(notices.len(), 1);
    let body = &notices[0].body;
    assert!(body.starts_with("<system-notice>\n"), "{body}");
    assert!(body.contains("src/lib.rs arrived after the edit"), "{body}");
    assert!(body.contains("untrusted data, not instructions"), "{body}");
    assert!(body.contains("LSP (1 error, 0 warnings)"), "{body}");
}

#[tokio::test(start_paused = true)]
async fn without_a_mailbox_the_report_still_tells_the_model_to_ask_again() {
    let (dir, lib) = project();
    let (manager, _) = fake_manager(dir.path(), late_server());

    let content = edited(&manager, None, &lib).await;

    assert!(
        content.contains("run lsp diagnostics path=src/lib.rs in a moment"),
        "{content}"
    );
}

#[tokio::test(start_paused = true)]
async fn a_file_edited_again_drops_the_late_notice() {
    let (dir, lib) = project();
    let (manager, _) = fake_manager(dir.path(), late_server());
    let inbox = crate::mailbox::Mailboxes::default().open("agent", None);
    edited(&manager, Some(&inbox), &lib).await;

    manager.files_changed(std::slice::from_ref(&lib));
    tokio::time::sleep(Duration::from_secs(20)).await;

    assert_eq!(inbox.pending(), 0);
}

#[tokio::test(start_paused = true)]
async fn late_diagnostics_equal_to_the_reported_ones_are_not_repeated() {
    let (dir, lib) = project();
    let busy = FakeOptions {
        indexing: true,
        ..FakeOptions::default()
    };
    let (manager, _) = fake_manager(dir.path(), busy);
    let inbox = crate::mailbox::Mailboxes::default().open("agent", None);

    let content = edited(&manager, Some(&inbox), &lib).await;
    assert!(
        content.starts_with("edited\n\nLSP so far (1 error, 0 warnings)"),
        "{content}"
    );
    tokio::time::sleep(Duration::from_secs(20)).await;

    assert_eq!(inbox.pending(), 0);
}

#[tokio::test(start_paused = true)]
async fn follow_up_waits_are_capped() {
    let (dir, lib) = project();
    let (manager, _) = fake_manager(dir.path(), late_server());
    let inbox = crate::mailbox::Mailboxes::default().open("agent", None);
    for _ in 0..deferred::MAX_FOLLOW_UPS + 3 {
        edited(&manager, Some(&inbox), &lib).await;
    }

    let running = manager.epochs.follow_ups.load(std::sync::atomic::Ordering::SeqCst);
    assert!(running <= deferred::MAX_FOLLOW_UPS, "{running}");
    tokio::time::sleep(Duration::from_secs(60)).await;
    assert_eq!(manager.epochs.follow_ups.load(std::sync::atomic::Ordering::SeqCst), 0);
}

fn watched(launcher: &test_server::FakeLauncher) -> Vec<Value> {
    let params = launcher.params.lock().unwrap();
    params
        .iter()
        .filter(|(method, _)| method == "workspace/didChangeWatchedFiles")
        .map(|(_, params)| params["changes"].clone())
        .collect()
}

#[tokio::test(start_paused = true)]
async fn a_check_that_starts_after_the_server_goes_idle_is_not_missed() {
    let (dir, lib) = project();
    let options = FakeOptions {
        check_on_save: true,
        ..FakeOptions::default()
    };
    let (manager, _) = fake_manager(dir.path(), options);

    let report = diagnostics_for(&manager, &lib, "ERR\n", Duration::from_secs(10))
        .await
        .unwrap();

    assert_eq!(report, "LSP (1 error, 0 warnings):\n1:1 error [fake] ERR found");
}

#[tokio::test(start_paused = true)]
async fn a_rewritten_open_file_is_resynced_so_stale_diagnostics_clear() {
    let (dir, lib) = project();
    let (manager, _) = fake_manager(dir.path(), FakeOptions::default());
    let wait = Duration::from_secs(3);
    let bad = diagnostics_for(&manager, &lib, "ERR\n", wait).await.unwrap();
    assert!(bad.contains("1 error"), "{bad}");

    std::fs::write(&lib, "fn ok() {}\n").unwrap();
    manager.refresh_after_write(std::slice::from_ref(&lib)).await;

    let client = manager.running_clients().remove(0);
    client
        .request("workspace/symbol", json!({ "query": "" }))
        .await
        .unwrap();
    let listed = client.state().all_published();
    assert!(
        listed.is_empty(),
        "the restored file must publish no diagnostics: {listed:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn servers_hear_about_files_rho_creates_changes_and_deletes() {
    let (dir, lib) = project();
    let (manager, launcher) = fake_manager(dir.path(), FakeOptions::default());
    let client = manager.client_for(&lib).await.unwrap();
    let made = dir.path().join("src/made.rs");
    let gone = dir.path().join("src/gone.rs");
    std::fs::write(&made, "").unwrap();

    manager.files_changed(&[lib.clone(), gone.clone()]);
    manager.files_created(std::slice::from_ref(&made));
    manager.files_changed(&[std::env::temp_dir().join("elsewhere.rs")]);
    client
        .request("workspace/symbol", json!({ "query": "" }))
        .await
        .unwrap();

    let uri = |path: &std::path::Path| {
        let parent = path.parent().unwrap().canonicalize().unwrap();
        client::file_uri(&parent.join(path.file_name().unwrap()))
    };
    let by_type = |changes: &Value, path: &std::path::Path| {
        changes
            .as_array()
            .unwrap()
            .iter()
            .find(|change| change["uri"] == uri(path))
            .map(|change| change["type"].clone())
    };
    let sent = watched(&launcher);
    assert_eq!(
        sent.len(),
        2,
        "nothing is sent for a file outside the server's root: {sent:?}"
    );
    assert_eq!(by_type(&sent[0], &lib), Some(json!(2)));
    assert_eq!(by_type(&sent[0], &gone), Some(json!(3)));
    assert_eq!(by_type(&sent[1], &made), Some(json!(1)));
    assert_eq!(manager.change_count(&lib), 1);
    assert_eq!(manager.change_count(&made), 1);
}

#[tokio::test(start_paused = true)]
async fn a_disk_change_found_while_syncing_open_documents_is_announced() {
    let (dir, lib) = project();
    let (manager, launcher) = fake_manager(dir.path(), FakeOptions::default());
    let client = manager.client_for(&lib).await.unwrap();
    let other = dir.path().join("src/other.rs");
    client.sync(&lib, "fn ok() {}\n", "rust").await.unwrap();
    client.sync(&other, "fn a() {}\n", "rust").await.unwrap();
    std::fs::write(&other, "fn a() {}\n").unwrap();

    client.sync_open_documents_from_disk().await.unwrap();
    client
        .request("workspace/symbol", json!({ "query": "" }))
        .await
        .unwrap();
    assert!(
        watched(&launcher).is_empty(),
        "documents equal to disk announce nothing"
    );

    std::fs::write(&lib, "fn changed() {}\n").unwrap();
    std::fs::remove_file(&other).unwrap();
    client.sync_open_documents_from_disk().await.unwrap();
    client
        .request("workspace/symbol", json!({ "query": "" }))
        .await
        .unwrap();

    let sent = watched(&launcher);
    assert_eq!(sent.len(), 1);
    let mut kinds: Vec<(String, u64)> = sent[0]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            (
                c["uri"].as_str().unwrap().rsplit('/').next().unwrap().to_string(),
                c["type"].as_u64().unwrap(),
            )
        })
        .collect();
    kinds.sort();
    assert_eq!(kinds, [("lib.rs".to_string(), 2), ("other.rs".to_string(), 3)]);
}

#[tokio::test(start_paused = true)]
async fn the_client_advertises_the_capabilities_the_new_actions_rely_on() {
    let (dir, lib) = project();
    let (manager, launcher) = fake_manager(dir.path(), FakeOptions::default());
    manager.client_for(&lib).await.unwrap();

    let params = launcher.params.lock().unwrap();
    let caps = &params.iter().find(|(method, _)| method == "initialize").unwrap().1["capabilities"];
    assert_eq!(caps["workspace"]["didChangeWatchedFiles"]["dynamicRegistration"], false);
    assert_eq!(
        caps["textDocument"]["codeAction"]["resolveSupport"]["properties"],
        json!(["edit"])
    );
    assert!(caps["textDocument"]["codeAction"]["codeActionLiteralSupport"].is_object());
    assert!(caps["textDocument"]["formatting"].is_object());
}
