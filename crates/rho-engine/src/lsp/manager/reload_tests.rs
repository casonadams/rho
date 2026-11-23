use super::*;
use crate::lsp::test_server::FakeLauncher;
use rho_test_support::TestDir;
use serde_json::json;

fn project() -> (TestDir, PathBuf) {
    super::tests::project()
}

fn write_lsp_json(path: &Path, text: &str, modified: SystemTime) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(modified)
        .unwrap();
}

fn reloading_manager(home: &Path, ws: &Path) -> LspManager {
    let launcher: Arc<dyn Launcher> = Arc::new(FakeLauncher::default());
    LspManager::new(ws, load_servers(Some(home), ws, true), None, launcher).with_reload(Some(home.to_path_buf()), true)
}

#[tokio::test(start_paused = true)]
async fn lsp_json_is_reloaded_when_a_file_changes_but_not_more_than_every_two_seconds() {
    let (dir, lib) = project();
    let home = TestDir::new();
    let file = home.as_ref().join(".agents/lsp.json");
    let epoch = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
    let manager = reloading_manager(home.as_ref(), dir.as_ref());
    assert!(manager.candidates(&lib).is_empty());

    let command = std::env::current_exe().unwrap();
    let config = json!({ "mine": { "command": command, "extensions": ["rs"], "rootMarkers": ["Cargo.toml"] } });
    write_lsp_json(&file, &config.to_string(), epoch);
    assert!(manager.candidates(&lib).is_empty(), "checked less than two seconds ago");

    tokio::time::advance(CONFIG_CHECK_INTERVAL).await;
    let names: Vec<String> = manager.candidates(&lib).into_iter().map(|c| c.server.name).collect();
    assert_eq!(names, ["mine"]);
    assert!(manager.servers().iter().any(|s| s.name == "mine"));
}

#[tokio::test(start_paused = true)]
async fn a_config_error_is_retried_and_clears_once_the_file_is_fixed() {
    let (dir, lib) = project();
    let home = TestDir::new();
    let file = home.as_ref().join(".agents/lsp.json");
    let epoch = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
    write_lsp_json(&file, "{\"x\":1}x", epoch);
    let manager = reloading_manager(home.as_ref(), dir.as_ref());
    assert!(manager.unavailable_reason(&lib).starts_with("lsp.json is invalid"));

    write_lsp_json(&file, "{\"x\":1} ", epoch);
    tokio::time::advance(CONFIG_CHECK_INTERVAL).await;
    assert!(!manager.unavailable_reason(&lib).contains("invalid"));
}

#[tokio::test(start_paused = true)]
async fn idle_timeout_ms_in_lsp_json_applies_after_a_reload() {
    let (dir, lib) = project();
    let home = TestDir::new();
    let epoch = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
    let manager = reloading_manager(home.as_ref(), dir.as_ref());
    assert_eq!(
        manager.idle_ms.load(Ordering::SeqCst),
        DEFAULT_IDLE_TIMEOUT.as_millis() as u64
    );

    write_lsp_json(
        &home.as_ref().join(".agents/lsp.json"),
        "{\"idleTimeoutMs\": 1234}",
        epoch,
    );
    tokio::time::advance(CONFIG_CHECK_INTERVAL).await;
    manager.candidates(&lib);
    assert_eq!(manager.idle_ms.load(Ordering::SeqCst), 1234);
}
