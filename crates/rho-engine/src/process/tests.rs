use super::*;

#[tokio::test]
#[cfg(unix)]
async fn guard_drop_terminates_the_whole_group() {
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg("sleep 30 & wait");
    isolate_group(&mut cmd);
    let child = cmd.spawn().expect("spawn test shell");
    let pid = child.id().expect("child pid");

    let guard = ProcessTreeGuard::new(child);
    assert_eq!(guard.id(), Some(pid));

    drop(guard);

    wait_group_dead(pid).await;
}

#[tokio::test]
#[cfg(unix)]
async fn guard_kill_and_wait_reaps_child() {
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg("sleep 30 & wait");
    isolate_group(&mut cmd);
    let child = cmd.spawn().expect("spawn test shell");
    let pid = child.id().expect("child pid");

    let mut guard = ProcessTreeGuard::new(child);
    let status = guard.kill_and_wait().await.expect("kill and wait succeeds");
    assert!(!status.success());

    wait_group_dead(pid).await;
}

#[tokio::test]
async fn guard_wait_untracks_pid_on_normal_exit() {
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg("exit 0");
    isolate_group(&mut cmd);
    let child = cmd.spawn().expect("spawn test shell");
    let pid = child.id().expect("child pid");

    let mut guard = ProcessTreeGuard::new(child);
    assert_eq!(guard.id(), Some(pid));

    let status = guard.wait().await.expect("wait succeeds");
    assert!(status.success());
    assert_eq!(guard.id(), None);
}

#[tokio::test]
#[cfg(unix)]
async fn drain_tasks_within_stops_waiting_for_a_grandchild_holding_the_pipes() {
    let cmd = configure_shell_command("sleep 30 & echo started", None);
    let (mut running, mut rx) = StreamingCommand::spawn(cmd).expect("spawn test shell");
    running.wait().await.expect("shell exits");

    running.drain_tasks_within(std::time::Duration::from_millis(50)).await;

    let mut output = String::new();
    while let Ok(chunk) = rx.try_recv() {
        output.push_str(&chunk);
    }
    assert!(output.contains("started"));
}
