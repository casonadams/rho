#[cfg(unix)]
#[tokio::test]
async fn panic_hook_kills_tracked_process_groups() {
    use rho_engine::process::{ProcessTreeGuard, install_panic_hook, isolate_group};
    use std::time::{Duration, Instant};

    let mut cmd = tokio::process::Command::new("sh");
    cmd.arg("-c").arg("sleep 30 & wait");
    isolate_group(&mut cmd);
    let child = cmd.spawn().expect("spawn test shell");
    let pid = child.id().expect("child pid");
    let mut guard = ProcessTreeGuard::new(child);

    install_panic_hook();
    let unwound = std::panic::catch_unwind(|| panic!("simulated crash"));
    assert!(unwound.is_err());

    let status = tokio::time::timeout(Duration::from_millis(2500), guard.wait())
        .await
        .expect("panic hook must kill the child")
        .expect("child reaped");
    assert!(!status.success());
    let deadline = Instant::now() + Duration::from_millis(2500);
    while unsafe { libc::kill(-(pid as libc::pid_t), 0) } != -1 {
        assert!(Instant::now() < deadline, "group {pid} survived the panic hook");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}
