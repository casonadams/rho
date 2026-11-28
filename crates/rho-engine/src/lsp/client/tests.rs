use super::*;
use crate::lsp::test_server::{
    FakeLauncher, FakeOptions, fake_manager, fake_manager_with, fake_server, project, until,
};
use rho_test_support::TestDir;
use test_case::test_case;

struct Fixture {
    dir: TestDir,
    launcher: Arc<FakeLauncher>,
    client: Arc<LspClient>,
}

impl Fixture {
    async fn new(options: FakeOptions) -> Self {
        let (dir, path) = project();
        let (manager, launcher) = fake_manager(dir.as_ref(), options);
        let client = manager.client_for(&path).await.unwrap();
        Self { dir, launcher, client }
    }

    fn lib(&self) -> PathBuf {
        self.dir.as_ref().join("src/lib.rs")
    }

    fn received(&self) -> Vec<String> {
        self.launcher.received.lock().unwrap().clone()
    }

    fn count(&self, method: &str) -> usize {
        self.received().iter().filter(|m| *m == method).count()
    }

    /// Returns once the server has handled everything sent so far.
    async fn settle(&self) {
        let _ = self.client.request("test/barrier", Value::Null).await;
    }
}

#[tokio::test(start_paused = true)]
async fn position_encoding_is_utf16_unless_the_server_offers_utf8() {
    let utf16 = Fixture::new(FakeOptions::default()).await;
    assert_eq!(utf16.client.encoding(), PositionEncoding::Utf16);
    let utf8 = Fixture::new(FakeOptions {
        utf8: true,
        ..FakeOptions::default()
    })
    .await;
    assert_eq!(utf8.client.encoding(), PositionEncoding::Utf8);
}

#[tokio::test(start_paused = true)]
async fn a_request_that_times_out_is_canceled_on_the_server() {
    let fixture = Fixture::new(FakeOptions {
        unresponsive: true,
        ..FakeOptions::default()
    })
    .await;
    let error = fixture
        .client
        .request("textDocument/hover", json!({}))
        .await
        .unwrap_err();
    assert!(error.contains("timed out"), "{error}");
    until("the cancel", || fixture.count("$/cancelRequest") > 0).await;
}

#[tokio::test(start_paused = true)]
async fn dynamically_registered_pull_diagnostics_are_pulled() {
    let fixture = Fixture::new(FakeOptions {
        dynamic_pull: true,
        ..FakeOptions::default()
    })
    .await;
    assert!(!fixture.client.supports("diagnosticProvider"));
    until("the pull registration", || {
        fixture.client.state().pull_registered.load(Ordering::SeqCst)
    })
    .await;
    let synced = fixture.client.sync(&fixture.lib(), "ERR here\n", "rust").await.unwrap();
    let outcome = fixture
        .client
        .diagnostics(&fixture.lib(), synced, Duration::from_secs(3))
        .await;
    let DiagnosticsOutcome::Ready(items) = outcome else {
        panic!("{outcome:?}");
    };
    assert_eq!(items.len(), 1);
    assert_eq!(fixture.count("textDocument/diagnostic"), 1);
}

#[tokio::test(start_paused = true)]
async fn a_failed_pull_falls_back_to_the_published_diagnostics() {
    let fixture = Fixture::new(FakeOptions {
        pull_fails: true,
        ..FakeOptions::default()
    })
    .await;
    assert!(fixture.client.supports("diagnosticProvider"));
    let synced = fixture.client.sync(&fixture.lib(), "ERR here\n", "rust").await.unwrap();
    let outcome = fixture
        .client
        .diagnostics(&fixture.lib(), synced, Duration::from_secs(3))
        .await;
    let DiagnosticsOutcome::Ready(items) = outcome else {
        panic!("{outcome:?}");
    };
    assert_eq!(items.len(), 1);
    assert_eq!(fixture.count("textDocument/diagnostic"), 1);
}

#[tokio::test(start_paused = true)]
async fn diagnostics_published_for_an_older_version_are_ignored() {
    let fixture = Fixture::new(FakeOptions {
        stale_version: true,
        ..FakeOptions::default()
    })
    .await;
    let synced = fixture.client.sync(&fixture.lib(), "ERR here\n", "rust").await.unwrap();
    let outcome = fixture
        .client
        .diagnostics(&fixture.lib(), synced, Duration::from_secs(1))
        .await;
    assert_eq!(outcome, DiagnosticsOutcome::Pending(None));
}

#[tokio::test(start_paused = true)]
async fn workspace_configuration_requests_are_answered_from_the_server_settings() {
    let (dir, path) = project();
    let mut server = fake_server("fake", false);
    server.settings = Some(json!({ "fake": { "opt": 5 } }));
    let options = FakeOptions {
        ask_configuration: true,
        ..FakeOptions::default()
    };
    let (manager, launcher) = fake_manager_with(dir.as_ref(), options, vec![server]);
    manager.client_for(&path).await.unwrap();
    until("the configuration answer", || {
        !launcher.responses.lock().unwrap().is_empty()
    })
    .await;
    assert_eq!(launcher.responses.lock().unwrap()[0], json!([5]));
}

#[tokio::test(start_paused = true)]
async fn syncing_unchanged_text_sends_nothing() {
    let fixture = Fixture::new(FakeOptions::default()).await;
    let first = fixture
        .client
        .sync(&fixture.lib(), "fn a() {}\n", "rust")
        .await
        .unwrap();
    let again = fixture
        .client
        .sync(&fixture.lib(), "fn a() {}\n", "rust")
        .await
        .unwrap();
    assert_eq!(first.version, again.version);
    fixture.settle().await;
    assert_eq!(fixture.count("textDocument/didOpen"), 1);
    assert_eq!(fixture.count("textDocument/didChange"), 0);

    let changed = fixture
        .client
        .sync(&fixture.lib(), "fn b() {}\n", "rust")
        .await
        .unwrap();
    assert_eq!(changed.version, first.version + 1);
}

#[tokio::test(start_paused = true)]
async fn unchanged_text_still_reports_the_diagnostics_published_since_its_last_change() {
    let fixture = Fixture::new(FakeOptions::default()).await;
    let synced = fixture.client.sync(&fixture.lib(), "ERR here\n", "rust").await.unwrap();
    let first = fixture
        .client
        .diagnostics(&fixture.lib(), synced, Duration::from_secs(3))
        .await;
    let again = fixture.client.sync(&fixture.lib(), "ERR here\n", "rust").await.unwrap();
    let second = fixture
        .client
        .diagnostics(&fixture.lib(), again, Duration::from_secs(3))
        .await;
    assert!(matches!(first, DiagnosticsOutcome::Ready(ref items) if items.len() == 1));
    assert_eq!(first, second);
}

#[tokio::test(start_paused = true)]
async fn documents_whose_file_is_gone_are_closed_when_syncing_from_disk() {
    let fixture = Fixture::new(FakeOptions::default()).await;
    let gone = fixture.dir.as_ref().join("src/gone.rs");
    std::fs::write(&gone, "fn gone() {}\n").unwrap();
    fixture.client.sync(&gone, "fn gone() {}\n", "rust").await.unwrap();
    fixture
        .client
        .sync(&fixture.lib(), "fn ok() {}\n", "rust")
        .await
        .unwrap();
    std::fs::remove_file(&gone).unwrap();

    fixture.client.sync_open_documents_from_disk().await.unwrap();
    fixture.settle().await;
    assert_eq!(fixture.count("textDocument/didClose"), 1);

    fixture.client.sync_open_documents_from_disk().await.unwrap();
    fixture.settle().await;
    assert_eq!(
        fixture.count("textDocument/didClose"),
        1,
        "a closed document stays closed"
    );
    assert_eq!(
        fixture.count("textDocument/didChange"),
        0,
        "unchanged files send nothing"
    );
}

#[tokio::test(start_paused = true)]
async fn close_document_closes_an_open_document_once() {
    let fixture = Fixture::new(FakeOptions::default()).await;
    fixture
        .client
        .sync(&fixture.lib(), "fn a() {}\n", "rust")
        .await
        .unwrap();
    fixture.client.close_document(&fixture.lib()).await.unwrap();
    fixture.client.close_document(&fixture.lib()).await.unwrap();
    fixture.settle().await;
    assert_eq!(fixture.count("textDocument/didClose"), 1);

    fixture
        .client
        .sync(&fixture.lib(), "fn a() {}\n", "rust")
        .await
        .unwrap();
    fixture.settle().await;
    assert_eq!(
        fixture.count("textDocument/didOpen"),
        2,
        "a closed document opens again"
    );
}

#[tokio::test(start_paused = true)]
async fn opening_more_than_the_cap_closes_the_least_recently_used_document() {
    let fixture = Fixture::new(FakeOptions::default()).await;
    let path = |index: usize| fixture.dir.as_ref().join(format!("src/f{index}.rs"));
    for index in 0..MAX_OPEN_DOCUMENTS {
        fixture.client.sync(&path(index), "fn a() {}\n", "rust").await.unwrap();
    }
    fixture.client.sync(&path(0), "fn a() {}\n", "rust").await.unwrap();
    fixture
        .client
        .sync(&path(MAX_OPEN_DOCUMENTS), "fn a() {}\n", "rust")
        .await
        .unwrap();
    fixture.settle().await;
    assert_eq!(fixture.count("textDocument/didClose"), 1);

    fixture.client.sync(&path(0), "fn a() {}\n", "rust").await.unwrap();
    fixture.settle().await;
    assert_eq!(
        fixture.count("textDocument/didOpen"),
        MAX_OPEN_DOCUMENTS + 1,
        "f0 was kept open"
    );
    fixture.client.sync(&path(1), "fn a() {}\n", "rust").await.unwrap();
    fixture.settle().await;
    assert_eq!(
        fixture.count("textDocument/didOpen"),
        MAX_OPEN_DOCUMENTS + 2,
        "f1, the least recently used, was closed"
    );
}

#[tokio::test(start_paused = true)]
async fn shutdown_asks_the_server_to_exit_and_stops_the_client() {
    let fixture = Fixture::new(FakeOptions::default()).await;
    fixture.client.shutdown().await;
    assert!(!fixture.client.is_alive());
    let received = fixture.received();
    let shutdown = received.iter().position(|m| m == "shutdown").unwrap();
    assert_eq!(received.get(shutdown + 1).map(String::as_str), Some("exit"));
}

#[tokio::test(start_paused = true)]
async fn shutdown_gives_up_on_a_server_that_never_answers() {
    let fixture = Fixture::new(FakeOptions {
        unresponsive: true,
        ..FakeOptions::default()
    })
    .await;
    let started = Instant::now();
    fixture.client.shutdown().await;
    let waited = started.elapsed();
    assert!(
        waited >= SHUTDOWN_REQUEST_TIMEOUT && waited < Duration::from_secs(4),
        "{waited:?}"
    );
}

impl Fixture {
    fn params(&self, method: &str) -> Vec<Value> {
        let params = self.launcher.params.lock().unwrap();
        params
            .iter()
            .filter(|(m, _)| m == method)
            .map(|(_, p)| p.clone())
            .collect()
    }
}

#[cfg(unix)]
#[tokio::test(start_paused = true)]
async fn one_file_reached_by_two_spellings_is_one_document() {
    let fixture = Fixture::new(FakeOptions::default()).await;
    let alias = fixture.dir.as_ref().join("alias");
    std::os::unix::fs::symlink(fixture.dir.as_ref().join("src"), &alias).unwrap();
    let through_alias = alias.join("lib.rs");

    fixture
        .client
        .sync(&fixture.lib(), "fn a() {}\n", "rust")
        .await
        .unwrap();
    fixture
        .client
        .sync(&through_alias, "fn a() {}\n", "rust")
        .await
        .unwrap();
    fixture.settle().await;

    assert_eq!(fixture.count("textDocument/didOpen"), 1);
    assert!(fixture.client.is_open(&through_alias).await);
    fixture.client.close_document(&through_alias).await.unwrap();
    assert!(!fixture.client.is_open(&fixture.lib()).await);
}

#[tokio::test(start_paused = true)]
async fn a_document_closed_while_syncing_from_disk_stays_closed() {
    let fixture = Fixture::new(FakeOptions::default()).await;
    let other = fixture.dir.as_ref().join("src/other.rs");
    std::fs::write(&other, "fn o() {}\n").unwrap();
    fixture
        .client
        .sync(&fixture.lib(), "fn ok() {}\n", "rust")
        .await
        .unwrap();
    fixture.client.sync(&other, "fn o() {}\n", "rust").await.unwrap();

    let (synced, closed) = tokio::join!(
        fixture.client.sync_open_documents_from_disk(),
        fixture.client.close_document(&other)
    );
    synced.unwrap();
    closed.unwrap();
    fixture.settle().await;

    assert!(
        !fixture.client.is_open(&other).await,
        "the closed document was reopened"
    );
    assert_eq!(fixture.count("textDocument/didOpen"), 2);
}

#[test_case(FakeOptions::default(), true, false; "save without text")]
#[test_case(FakeOptions { save_text: true, ..FakeOptions::default() }, true, true; "save with text")]
#[test_case(FakeOptions { no_save: true, ..FakeOptions::default() }, false, false; "no save capability")]
#[tokio::test(start_paused = true)]
async fn did_save_follows_the_servers_save_capability(options: FakeOptions, sent: bool, with_text: bool) {
    let fixture = Fixture::new(options).await;
    fixture
        .client
        .sync(&fixture.lib(), "fn a() {}\n", "rust")
        .await
        .unwrap();
    fixture.client.did_save(&fixture.lib(), "fn a() {}\n").await.unwrap();
    fixture.settle().await;
    let saves = fixture.params("textDocument/didSave");
    assert_eq!(saves.len(), usize::from(sent));
    assert!(saves.iter().all(|p| p.get("text").is_some() == with_text), "{saves:?}");
}

#[tokio::test(start_paused = true)]
async fn closing_a_document_forgets_its_published_diagnostics() {
    let fixture = Fixture::new(FakeOptions::default()).await;
    let synced = fixture.client.sync(&fixture.lib(), "ERR here\n", "rust").await.unwrap();
    fixture
        .client
        .diagnostics(&fixture.lib(), synced, Duration::from_secs(3))
        .await;
    assert!(!fixture.client.state().all_published().is_empty());

    fixture.client.close_document(&fixture.lib()).await.unwrap();

    assert!(fixture.client.state().published_len() == 0);
}
