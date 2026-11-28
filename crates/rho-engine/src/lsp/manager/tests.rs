use super::*;
use crate::lsp::config::ServerSet;
use crate::lsp::test_server::{
    FakeLauncher, FakeOptions, fake_manager, fake_manager_with, fake_server, project, until,
};
use rho_test_support::TestDir;
use std::sync::atomic::AtomicBool;

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
    until("the launch", || launcher.launches.load(Ordering::SeqCst) >= count).await;
}

fn count(launcher: &FakeLauncher, method: &str) -> usize {
    launcher
        .received
        .lock()
        .unwrap()
        .iter()
        .filter(|m| *m == method)
        .count()
}

async fn until_received(launcher: &FakeLauncher, method: &str, times: usize) {
    until(method, || count(launcher, method) >= times).await;
}

/// Removes a workspace's manager from the process-wide registry when the test ends.
struct Unregister(PathBuf);

impl Drop for Unregister {
    fn drop(&mut self) {
        drop(forget_workspace(&self.0));
    }
}

#[tokio::test(start_paused = true)]
async fn an_untrusted_workspace_starts_servers_only_on_explicit_request() {
    let (dir, lib) = project();
    let (manager, launcher) = manager_with(
        dir.as_ref(),
        FakeOptions::default(),
        set(vec![fake_server("fake", false)]),
    );
    let manager = Arc::new(manager);

    manager.warm(&lib);
    assert!(manager.clients_within(&lib, Duration::from_secs(1)).await.is_empty());
    let edited = crate::tools::types::ToolResult::success("edited");
    let after_edit = crate::lsp::append_diagnostics(edited, Some(&manager), None, &lib, "ERR\n", false).await;
    assert_eq!(after_edit.content, "edited", "an edit alone must not start a server");
    tokio::task::yield_now().await;
    assert_eq!(launcher.launches.load(Ordering::SeqCst), 0);

    let asked = crate::lsp::diagnostics_for(&manager, &lib, "ERR\n", Duration::from_secs(1))
        .await
        .expect("an explicit diagnostics request starts the server");
    assert!(asked.contains("1 error"), "{asked}");
    assert_eq!(launcher.launches.load(Ordering::SeqCst), 1);

    let client = manager.client_for(&lib).await.unwrap();
    let within = manager.clients_within(&lib, Duration::from_secs(1)).await;
    assert_eq!(launcher.launches.load(Ordering::SeqCst), 1);
    assert!(matches!(within.as_slice(), [Ok(found)] if Arc::ptr_eq(found, &client)));
}

#[tokio::test(start_paused = true)]
async fn a_trusted_workspace_warms_servers() {
    let (dir, lib) = project();
    let (manager, launcher) = manager_with(
        dir.as_ref(),
        FakeOptions::default(),
        set(vec![fake_server("fake", false)]),
    );
    let manager = manager.with_trust(true);
    manager.warm(&lib);
    launched(&launcher, 1).await;
}

#[tokio::test(start_paused = true)]
async fn concurrent_warms_and_lookups_share_one_start() {
    let (dir, lib) = project();
    let (manager, launcher) = fake_manager(dir.as_ref(), FakeOptions::default());
    manager.warm(&lib);
    manager.warm(&lib);
    let (first, second) = tokio::join!(
        manager.client_for(&lib),
        manager.clients_within(&lib, Duration::from_secs(1))
    );
    let first = first.unwrap();
    assert!(matches!(second.as_slice(), [Ok(found)] if Arc::ptr_eq(found, &first)));
    assert_eq!(launcher.launches.load(Ordering::SeqCst), 1);
}

#[test]
fn the_main_server_comes_before_linters_however_they_are_listed() {
    let (dir, lib) = project();
    let (manager, _) = fake_manager_with(
        dir.as_ref(),
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
    let broken = LspManager::new(
        dir.as_ref(),
        Err("expected value at line 1".into()),
        None,
        Arc::clone(&launcher),
    );
    assert_eq!(
        broken.unavailable_reason(&lib),
        "lsp.json is invalid: expected value at line 1"
    );

    let mut missing = fake_server("ghost", false);
    missing.command = "definitely-not-installed-lsp".into();
    let manager = LspManager::new(dir.as_ref(), Ok(set(vec![missing])), None, Arc::clone(&launcher));
    assert_eq!(
        manager.unavailable_reason(&lib),
        "no language server available: ghost: `definitely-not-installed-lsp` is not on PATH"
    );

    let manager = LspManager::new(dir.as_ref(), Ok(set(vec![fake_server("fake", false)])), None, launcher);
    std::fs::remove_file(dir.as_ref().join("Cargo.toml")).unwrap();
    assert_eq!(
        manager.unavailable_reason(&lib),
        "no language server available: fake: no root marker (Cargo.toml) above the file"
    );
    assert_eq!(
        manager.unavailable_reason(&dir.as_ref().join("notes.txt")),
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
    let (manager, launcher) = fake_manager(dir.as_ref(), options);
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
    let (manager, launcher) = fake_manager(dir.as_ref(), options);
    for round in 1..=2 {
        let client = manager.client_for(&lib).await.unwrap();
        client.sync(&lib, "fn a() {}\n", "rust").await.unwrap();
        client.sync(&lib, "fn b() {}\n", "rust").await.unwrap();
        until("the server to exit", || !client.is_alive()).await;
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
    let (manager, _) = fake_manager(dir.as_ref(), options);
    let client = manager.client_for(&lib).await.unwrap();
    client.sync(&lib, "fn a() {}\n", "rust").await.unwrap();
    client.sync(&lib, "fn b() {}\n", "rust").await.unwrap();
    until("the server to exit", || !client.is_alive()).await;
    assert!(manager.status_lines()[0].ends_with("exited"));
    tokio::time::advance(RETRY_FAILED_AFTER).await;
    assert!(manager.status_lines().is_empty());
}

#[tokio::test(start_paused = true)]
async fn servers_unused_for_ten_minutes_are_shut_down() {
    let (dir, lib) = project();
    let (manager, launcher) = fake_manager(dir.as_ref(), FakeOptions::default());
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
        dir.as_ref(),
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
    let dir = TestDir::new();
    let (manager, launcher) = fake_manager(dir.as_ref(), FakeOptions::default());
    let mut roots = Vec::new();
    for index in 0..=MAX_RUNNING_SERVERS {
        let root = dir.as_ref().join(format!("crate{index}"));
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
    assert!(
        status.contains(roots[MAX_RUNNING_SERVERS].to_str().unwrap()),
        "{status}"
    );
}

#[tokio::test(start_paused = true)]
async fn a_burst_of_starts_never_exceeds_the_cap() {
    let dir = TestDir::new();
    let (manager, _) = fake_manager(dir.as_ref(), FakeOptions::default());
    for index in 0..MAX_RUNNING_SERVERS + 3 {
        let root = dir.as_ref().join(format!("crate{index}"));
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("Cargo.toml"), "").unwrap();
        let file = root.join("src/lib.rs");
        std::fs::write(&file, "").unwrap();
        manager.warm(&file);
    }
    let slots = manager.status_lines();
    assert!(slots.len() <= MAX_RUNNING_SERVERS, "{} slots: {slots:?}", slots.len());
}

#[test]
fn the_idle_reaper_restarts_after_its_runtime_died() {
    let (dir, lib) = project();
    let (manager, _) = fake_manager(dir.as_ref(), FakeOptions::default());
    let runtime = || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .start_paused(true)
            .build()
            .unwrap()
    };
    let first = runtime();
    first.block_on(async { manager.client_for(&lib).await.unwrap() });
    drop(first);

    runtime().block_on(async {
        let client = manager.client_for(&lib).await.unwrap();
        tokio::time::sleep(DEFAULT_IDLE_TIMEOUT + REAP_INTERVAL * 2).await;
        assert!(!client.is_alive(), "nothing reaped the idle server");
    });
}

#[tokio::test(start_paused = true)]
async fn shutdown_all_stops_every_running_server() {
    let (dir, lib) = project();
    let (manager, launcher) = fake_manager(dir.as_ref(), FakeOptions::default());
    let client = manager.client_for(&lib).await.unwrap();
    manager.shutdown_all().await;
    assert!(!client.is_alive());
    assert_eq!(count(&launcher, "shutdown"), 1);
    assert_eq!(count(&launcher, "exit"), 1);
    assert!(manager.status_lines().is_empty());
}

#[tokio::test(start_paused = true)]
async fn shutdown_workspace_stops_its_servers_and_forgets_the_manager() {
    let (dir, lib) = project();
    let (manager, launcher) = fake_manager(dir.as_ref(), FakeOptions::default());
    register_manager(&manager);
    let client = manager.client_for(&lib).await.unwrap();
    assert!(is_registered(dir.as_ref()));
    shutdown_workspace(dir.as_ref()).await;
    assert!(!client.is_alive());
    assert_eq!(count(&launcher, "shutdown"), 1);
    assert!(!is_registered(dir.as_ref()));
    shutdown_workspace(dir.as_ref()).await;
}

#[tokio::test(start_paused = true)]
async fn shutdown_all_is_bounded_when_a_server_hangs() {
    let (dir, lib) = project();
    let options = FakeOptions {
        unresponsive: true,
        ..FakeOptions::default()
    };
    let (manager, _) = fake_manager(dir.as_ref(), options);
    manager.client_for(&lib).await.unwrap();
    let started = Instant::now();
    manager.shutdown_all().await;
    assert!(started.elapsed() <= SHUTDOWN_BUDGET);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_exit_hook_shuts_down_the_servers_of_registered_workspaces() {
    let (dir, lib) = project();
    let (manager, launcher) = fake_manager(dir.as_ref(), FakeOptions::default());
    let registered = managed(dir.as_ref(), || Arc::clone(&manager));
    let _cleanup = Unregister(dir.as_ref().to_path_buf());
    manager.client_for(&lib).await.unwrap();

    shutdown_all_blocking();
    assert_eq!(count(&launcher, "exit"), 1);
    assert!(registered.running_clients().is_empty());
}

/// Real process group behind the fake server's stdio, wrapped as `ProcessLauncher` wraps its child.
struct GroupLauncher {
    fake: FakeLauncher,
    group: Arc<Mutex<Option<u32>>>,
}

impl Launcher for GroupLauncher {
    fn launch(&self, server: &ServerConfig, command: &Path, root: &Path) -> Result<Launched, String> {
        let mut launched = self.fake.launch(server, command, root)?;
        let mut cmd = tokio::process::Command::new("sh");
        cmd.arg("-c").arg("sleep 31 & wait").kill_on_drop(true);
        crate::process::isolate_group(&mut cmd);
        let guard = crate::process::ProcessTreeGuard::for_session(cmd.spawn().map_err(|e| e.to_string())?);
        *self.group.lock().unwrap() = guard.id();
        launched.process = Some(Box::new(guard));
        Ok(launched)
    }
}

fn group_manager(dir: &Path) -> (Arc<LspManager>, Arc<Mutex<Option<u32>>>) {
    let group = Arc::new(Mutex::new(None));
    let launcher = GroupLauncher {
        fake: FakeLauncher::default(),
        group: Arc::clone(&group),
    };
    let manager = LspManager::new(dir, Ok(set(vec![fake_server("fake", false)])), None, Arc::new(launcher));
    (Arc::new(manager.with_trust(true)), group)
}

#[cfg(unix)]
#[tokio::test]
async fn forgetting_a_workspace_kills_its_servers_process_group() {
    let (dir, lib) = project();
    let (manager, group) = group_manager(dir.as_ref());
    register_manager(&manager);
    let client = manager.client_for(&lib).await.unwrap();
    let pid = group.lock().unwrap().expect("the server process started");
    assert!(crate::process::is_pid_tracked(pid));

    drop(client);
    drop(forget_workspace(dir.as_ref()));
    drop(manager);

    crate::process::wait_group_dead(pid).await;
    assert!(!crate::process::is_pid_tracked(pid));
}

#[cfg(unix)]
#[tokio::test]
async fn shutting_a_server_down_kills_its_process_group() {
    let (dir, lib) = project();
    let (manager, group) = group_manager(dir.as_ref());
    let client = manager.client_for(&lib).await.unwrap();
    let pid = group.lock().unwrap().expect("the server process started");

    manager.shutdown_all().await;

    assert!(!client.is_alive());
    crate::process::wait_group_dead(pid).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn blocking_shutdown_from_a_runtime_task_returns_as_soon_as_the_work_does() {
    let finished = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&finished);
    tokio::spawn(async move {
        block_on_runtime(
            async move { flag.store(true, Ordering::SeqCst) },
            Duration::from_secs(30),
        )
    })
    .await
    .unwrap();

    assert!(finished.load(Ordering::SeqCst), "the work ran before the call returned");
}

#[test]
fn one_workspace_has_one_manager_however_its_path_is_spelled() {
    let (dir, _) = project();
    let (manager, _) = fake_manager(dir.as_ref(), FakeOptions::default());
    let spelled = dir.as_ref().join("src/..");
    let first = managed(&spelled, || Arc::clone(&manager));
    let _cleanup = Unregister(dir.as_ref().to_path_buf());
    let second = managed(dir.as_ref(), || panic!("a second manager was made"));
    assert!(Arc::ptr_eq(&first, &second));
}

#[cfg(unix)]
#[test]
fn files_reached_through_a_symlink_out_of_the_workspace_are_outside_it() {
    let (dir, lib) = project();
    let outside = TestDir::new();
    std::fs::write(outside.as_ref().join("dep.rs"), "").unwrap();
    std::os::unix::fs::symlink(outside.as_ref(), dir.as_ref().join("vendor")).unwrap();
    let (manager, _) = fake_manager(dir.as_ref(), FakeOptions::default());

    assert!(manager.in_workspace(&lib));
    assert!(
        manager.in_workspace(&dir.as_ref().join("src/new.rs")),
        "new files resolve through their directory"
    );
    assert!(!manager.in_workspace(&dir.as_ref().join("vendor/dep.rs")));
    assert!(!manager.in_workspace(&dir.as_ref().join("vendor/new.rs")));
    assert!(!manager.in_workspace(&dir.as_ref().join("src/../../outside.rs")));
}
