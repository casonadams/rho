use super::*;
use crate::lsp::test_server::{FakeOptions, fake_manager, fake_manager_with, fake_server};

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
