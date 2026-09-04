use rho_engine::process::{ProcessTreeGuard, isolate_group, kill_all_tracked_processes};
use std::time::{Duration, Instant};
use tokio::process::Command;

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
async fn kill_all_tracked_processes_terminates_all_groups() {
    let mut cmd1 = Command::new("sh");
    cmd1.arg("-c").arg("sleep 30 & wait");
    isolate_group(&mut cmd1);
    let child1 = cmd1.spawn().expect("spawn test shell 1");
    let pid1 = child1.id().expect("child 1 pid");
    let mut guard1 = ProcessTreeGuard::new(child1);

    let mut cmd2 = Command::new("sh");
    cmd2.arg("-c").arg("sleep 30 & wait");
    isolate_group(&mut cmd2);
    let child2 = cmd2.spawn().expect("spawn test shell 2");
    let pid2 = child2.id().expect("child 2 pid");
    let mut guard2 = ProcessTreeGuard::new(child2);

    kill_all_tracked_processes();

    let _ = guard1.wait().await;
    let _ = guard2.wait().await;

    wait_group_dead(pid1).await;
    wait_group_dead(pid2).await;
}
