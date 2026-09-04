use super::*;
use std::time::{Duration, Instant};

#[cfg(unix)]
async fn wait_group_dead(pid: u32) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if unsafe { libc::kill(-(pid as libc::pid_t), 0) } == -1 {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("process group {pid} still has living members");
}

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

#[tokio::test]
#[cfg(unix)]
async fn kill_all_tracked_processes_terminates_all_groups() {
    let mut cmd1 = Command::new("sh");
    cmd1.arg("-c").arg("sleep 30 & wait");
    isolate_group(&mut cmd1);
    let child1 = cmd1.spawn().expect("spawn test shell 1");
    let pid1 = child1.id().expect("child 1 pid");
    let _guard1 = ProcessTreeGuard::new(child1);

    let mut cmd2 = Command::new("sh");
    cmd2.arg("-c").arg("sleep 30 & wait");
    isolate_group(&mut cmd2);
    let child2 = cmd2.spawn().expect("spawn test shell 2");
    let pid2 = child2.id().expect("child 2 pid");
    let _guard2 = ProcessTreeGuard::new(child2);

    kill_all_tracked_processes();

    wait_group_dead(pid1).await;
    wait_group_dead(pid2).await;
}
