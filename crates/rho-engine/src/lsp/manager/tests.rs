use super::*;
use crate::lsp::config::ServerSet;
use crate::lsp::test_server::{FakeLauncher, FakeOptions, fake_manager, fake_manager_with, fake_server};
use serde_json::json;

fn project() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Cargo.toml"), "").unwrap();
    std::fs::create_dir(dir.path().join("src")).unwrap();
    let lib = dir.path().join("src/lib.rs");
    std::fs::write(&lib, "fn ok() {}\n").unwrap();
    (dir, lib)
}

fn manager_with(dir: &Path, options: FakeOptions, set: ServerSet) -> (LspManager, Arc<FakeLauncher>) {
    let launcher = Arc::new(FakeLauncher {
        options,
        ..FakeLauncher::default()
    });
    let manager = LspManager::new(dir, Ok(set), None, Arc::clone(&launcher) as Arc<dyn Launcher>);
    (manager, launcher)
}

fn set(servers: Vec<ServerConfig>) -> ServerSet {
    ServerSet {
        servers,
        ..ServerSet::default()
    }
}

async fn launched(launcher: &FakeLauncher, count: usize) {
    while launcher.launches.load(Ordering::SeqCst) < count {
        tokio::task::yield_now().await;
    }
}

fn count(launcher: &FakeLauncher, method: &str) -> usize {
    launcher.received.lock().unwrap().iter().filter(|m| *m == method).count()
}

async fn until_received(launcher: &FakeLauncher, method: &str, times: usize) {
    while count(launcher, method) < times {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test(start_paused = true)]
async fn an_untrusted_workspace_starts_servers_only_on_explicit_request() {
    let (dir, lib) = project();
    let (manager, launcher) = manager_with(dir.path(), FakeOptions::default(), set(vec![fake_server("fake", false)]));

    manager.warm(&lib);
    assert!(manager.clients_within(&lib, Duration::from_secs(1)).await.is_empty());
    assert!(crate::lsp::diagnostics_for(&manager, &lib, "ERR\n", Duration::from_secs(1)).await.is_none());
    tokio::task::yield_now().await;
    assert_eq!(launcher.launches.load(Ordering::SeqCst), 0);

    let client = manager.client_for(&lib).await.unwrap();
    let within = manager.clients_within(&lib, Duration::from_secs(1)).await;
    assert_eq!(launcher.launches.load(Ordering::SeqCst), 1);
    assert!(matches!(within.as_slice(), [Ok(found)] if Arc::ptr_eq(found, &client)));
}

#[tokio::test(start_paused = true)]
async fn a_trusted_workspace_warms_servers() {
    let (dir, lib) = project();
    let (manager, launcher) = manager_with(dir.path(), FakeOptions::default(), set(vec![fake_server("fake", false)]));
    let manager = manager.with_trust(true);
    manager.warm(&lib);
    launched(&launcher, 1).await;
}

#[tokio::test(start_paused = true)]
async fn concurrent_warms_and_lookups_share_one_start() {
    let (dir, lib) = project();
    let (manager, launcher) = fake_manager(dir.path(), FakeOptions::default());
    manager.warm(&lib);
    manager.warm(&lib);
    let (first, second) = tokio::join!(manager.client_for(&lib), manager.clients_within(&lib, Duration::from_secs(1)));
    let first = first.unwrap();
    assert!(matches!(second.as_slice(), [Ok(found)] if Arc::ptr_eq(found, &first)));
    assert_eq!(launcher.launches.load(Ordering::SeqCst), 1);
}

#[test]
fn the_main_server_comes_before_linters_however_they_are_listed() {
    let (dir, lib) = project();
    let (manager, _) = fake_manager_with(
        dir.path(),
        FakeOptions::default(),
        vec![fake_server("lint", true), fake_server("main", false)],
    );
    let names: Vec<String> = manager.candidates(&lib).into_iter().map(|c| c.server.name).collect();
    assert_eq!(names, ["main", "lint"]);
}

#[test]
fn unavailable_reasons_say_what_is_wrong() {
    let (dir, lib) = project();
    let launcher: Arc<dyn Launcher> = Arc::new(FakeLauncher::default());
    let broken = LspManager::new(dir.path(), Err("expected value at line 1".into()), None, Arc::clone(&launcher));
    assert_eq!(
        broken.unavailable_reason(&lib),
        "lsp.json is invalid: expected value at line 1"
    );

    let mut missing = fake_server("ghost", false);
    missing.command = "definitely-not-installed-lsp".into();
    let manager = LspManager::new(dir.path(), Ok(set(vec![missing])), None, Arc::clone(&launcher));
    assert_eq!(
        manager.unavailable_reason(&lib),
        "no language server available: ghost: `definitely-not-installed-lsp` is not on PATH"
    );

    let manager = LspManager::new(dir.path(), Ok(set(vec![fake_server("fake", false)])), None, launcher);
    std::fs::remove_file(dir.path().join("Cargo.toml")).unwrap();
    assert_eq!(
        manager.unavailable_reason(&lib),
        "no language server available: fake: no root marker (Cargo.toml) above the file"
    );
    assert_eq!(
        manager.unavailable_reason(&dir.path().join("notes.txt")),
        "no language server is configured for '.txt' files"
    );
}

#[tokio::test(start_paused = true)]
async fn a_server_still_starting_at_the_deadline_names_the_follow_up_call() {
    let (dir, lib) = project();
    let options = FakeOptions {
        hang_init: true,
        ..FakeOptions::default()
    };
    let (manager, launcher) = fake_manager(dir.path(), options);
    let report = crate::lsp::diagnostics_for(&manager, &lib, "ERR\n", Duration::from_secs(3))
        .await
        .unwrap();
    assert_eq!(
        report,
        "LSP: no diagnostics yet\n[fake is still starting; run lsp diagnostics path=src/lib.rs in a moment]"
    );
    assert_eq!(launcher.launches.load(Ordering::SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn an_exited_server_restarts_once_at_once_then_backs_off() {
    let (dir, lib) = project();
    let options = FakeOptions {
        exit_on_change: true,
        ..FakeOptions::default()
    };
    let (manager, launcher) = fake_manager(dir.path(), options);
    for round in 1..=2 {
        let client = manager.client_for(&lib).await.unwrap();
        client.sync(&lib, "fn a() {}\n", "rust").await.unwrap();
        client.sync(&lib, "fn b() {}\n", "rust").await.unwrap();
        while client.is_alive() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(launcher.launches.load(Ordering::SeqCst), round);
    }
    let error = manager.client_for(&lib).await.err().unwrap();
    assert!(error.contains("not restarting"), "{error}");
    assert_eq!(launcher.launches.load(Ordering::SeqCst), 2);

    tokio::time::advance(RETRY_FAILED_AFTER).await;
    manager.client_for(&lib).await.unwrap();
    assert_eq!(launcher.launches.load(Ordering::SeqCst), 3);
}

#[tokio::test(start_paused = true)]
async fn status_forgets_servers_that_exited_long_ago() {
    let (dir, lib) = project();
    let options = FakeOptions {
        exit_on_change: true,
        ..FakeOptions::default()
    };
    let (manager, _) = fake_manager(dir.path(), options);
    let client = manager.client_for(&lib).await.unwrap();
    client.sync(&lib, "fn a() {}\n", "rust").await.unwrap();
    client.sync(&lib, "fn b() {}\n", "rust").await.unwrap();
    while client.is_alive() {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(manager.status_lines()[0].ends_with("exited"));
    tokio::time::advance(RETRY_FAILED_AFTER).await;
    assert!(manager.status_lines().is_empty());
}

#[tokio::test(start_paused = true)]
async fn servers_unused_for_ten_minutes_are_shut_down() {
    let (dir, lib) = project();
    let (manager, launcher) = fake_manager(dir.path(), FakeOptions::default());
    let client = manager.client_for(&lib).await.unwrap();

    tokio::time::sleep(DEFAULT_IDLE_TIMEOUT / 2).await;
    assert!(client.is_alive());
    manager.client_for(&lib).await.unwrap();
    tokio::time::sleep(DEFAULT_IDLE_TIMEOUT / 2 + REAP_INTERVAL).await;
    assert!(client.is_alive(), "using the server restarts its idle clock");

    tokio::time::sleep(DEFAULT_IDLE_TIMEOUT).await;
    until_received(&launcher, "exit", 1).await;
    assert!(manager.running_clients().is_empty());
    assert!(manager.status_lines().is_empty());
}

#[tokio::test(start_paused = true)]
async fn a_zero_idle_timeout_keeps_servers_running() {
    let (dir, lib) = project();
    let (manager, _) = manager_with(
        dir.path(),
        FakeOptions::default(),
        ServerSet {
            idle_timeout_ms: Some(0),
            ..set(vec![fake_server("fake", false)])
        },
    );
    let client = manager.client_for(&lib).await.unwrap();
    tokio::time::sleep(DEFAULT_IDLE_TIMEOUT * 3).await;
    assert!(client.is_alive());
}

#[tokio::test(start_paused = true)]
async fn starting_more_servers_than_the_cap_stops_the_least_recently_used() {
    let dir = tempfile::tempdir().unwrap();
    let (manager, launcher) = fake_manager(dir.path(), FakeOptions::default());
    let mut roots = Vec::new();
    for index in 0..=MAX_RUNNING_SERVERS {
        let root = dir.path().join(format!("crate{index}"));
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("Cargo.toml"), "").unwrap();
        let file = root.join("src/lib.rs");
        std::fs::write(&file, "").unwrap();
        manager.client_for(&file).await.unwrap();
        tokio::time::advance(Duration::from_secs(1)).await;
        roots.push(manager.workspace().join(format!("crate{index}")));
    }
    until_received(&launcher, "exit", 1).await;
    assert_eq!(manager.running_clients().len(), MAX_RUNNING_SERVERS);
    let status = manager.status_lines().join("\n");
    assert!(!status.contains(roots[0].to_str().unwrap()), "{status}");
    assert!(status.contains(roots[MAX_RUNNING_SERVERS].to_str().unwrap()), "{status}");
}

#[tokio::test(start_paused = true)]
async fn shutdown_all_stops_every_running_server() {
    let (dir, lib) = project();
    let (manager, launcher) = fake_manager(dir.path(), FakeOptions::default());
    let client = manager.client_for(&lib).await.unwrap();
    manager.shutdown_all().await;
    assert!(!client.is_alive());
    assert_eq!(count(&launcher, "shutdown"), 1);
    assert_eq!(count(&launcher, "exit"), 1);
    assert!(manager.status_lines().is_empty());
}

#[tokio::test(start_paused = true)]
async fn shutdown_all_is_bounded_when_a_server_hangs() {
    let (dir, lib) = project();
    let options = FakeOptions {
        unresponsive: true,
        ..FakeOptions::default()
    };
    let (manager, _) = fake_manager(dir.path(), options);
    manager.client_for(&lib).await.unwrap();
    let started = Instant::now();
    manager.shutdown_all().await;
    assert!(started.elapsed() <= SHUTDOWN_BUDGET);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_exit_hook_shuts_down_the_servers_of_registered_workspaces() {
    let (dir, lib) = project();
    let (manager, launcher) = fake_manager(dir.path(), FakeOptions::default());
    let registered = managed(dir.path(), || Arc::clone(&manager));
    manager.client_for(&lib).await.unwrap();

    shutdown_all_blocking();
    assert_eq!(count(&launcher, "exit"), 1);
    assert!(registered.running_clients().is_empty());
}

#[test]
fn one_workspace_has_one_manager_however_its_path_is_spelled() {
    let (dir, _) = project();
    let (manager, _) = fake_manager(dir.path(), FakeOptions::default());
    let spelled = dir.path().join("src/..");
    let first = managed(&spelled, || Arc::clone(&manager));
    let second = managed(dir.path(), || panic!("a second manager was made"));
    assert!(Arc::ptr_eq(&first, &second));
}

#[cfg(unix)]
#[test]
fn files_reached_through_a_symlink_out_of_the_workspace_are_outside_it() {
    let (dir, lib) = project();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("dep.rs"), "").unwrap();
    std::os::unix::fs::symlink(outside.path(), dir.path().join("vendor")).unwrap();
    let (manager, _) = fake_manager(dir.path(), FakeOptions::default());

    assert!(manager.in_workspace(&lib));
    assert!(manager.in_workspace(&dir.path().join("src/new.rs")), "new files resolve through their directory");
    assert!(!manager.in_workspace(&dir.path().join("vendor/dep.rs")));
    assert!(!manager.in_workspace(&dir.path().join("vendor/new.rs")));
    assert!(!manager.in_workspace(&dir.path().join("src/../../outside.rs")));
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
    let home = tempfile::tempdir().unwrap();
    let file = home.path().join(".agents/lsp.json");
    let epoch = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
    let manager = reloading_manager(home.path(), dir.path());
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
    let home = tempfile::tempdir().unwrap();
    let file = home.path().join(".agents/lsp.json");
    let epoch = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
    write_lsp_json(&file, "{\"x\":1}x", epoch);
    let manager = reloading_manager(home.path(), dir.path());
    assert!(manager.unavailable_reason(&lib).starts_with("lsp.json is invalid"));

    write_lsp_json(&file, "{\"x\":1} ", epoch);
    tokio::time::advance(CONFIG_CHECK_INTERVAL).await;
    assert!(!manager.unavailable_reason(&lib).contains("invalid"));
}

#[tokio::test(start_paused = true)]
async fn idle_timeout_ms_in_lsp_json_applies_after_a_reload() {
    let (dir, lib) = project();
    let home = tempfile::tempdir().unwrap();
    let epoch = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
    let manager = reloading_manager(home.path(), dir.path());
    assert_eq!(manager.idle_ms.load(Ordering::SeqCst), DEFAULT_IDLE_TIMEOUT.as_millis() as u64);

    write_lsp_json(&home.path().join(".agents/lsp.json"), "{\"idleTimeoutMs\": 1234}", epoch);
    tokio::time::advance(CONFIG_CHECK_INTERVAL).await;
    manager.candidates(&lib);
    assert_eq!(manager.idle_ms.load(Ordering::SeqCst), 1234);
}
