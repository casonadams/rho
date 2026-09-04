use super::*;

#[tokio::test]
#[cfg(unix)]
async fn kill_tree_terminates_the_whole_group() {
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg("sleep 30 & wait");
    isolate_group(&mut cmd);
    let mut child = cmd.spawn().expect("spawn test shell");
    let pid = child.id().expect("child pid");

    kill_tree(&mut child).await;
    child.wait().await.expect("reap child");

    wait_group_dead(pid).await;
}

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
async fn guard_disarm_preserves_process_and_untracks() {
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg("exit 42");
    isolate_group(&mut cmd);
    let child = cmd.spawn().expect("spawn test shell");
    let pid = child.id().expect("child pid");

    let guard = ProcessTreeGuard::new(child);
    assert_eq!(guard.id(), Some(pid));
    let mut child = guard.disarm().expect("disarmed child");

    let status = child.wait().await.expect("reap child");
    assert_eq!(status.code(), Some(42));
}
