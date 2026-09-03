//! POSIX process-group isolation so a kill reaches the entire command tree.

use tokio::process::Command;

/// Places the child in its own process group so one group kill reaches every
/// descendant. `sh -c` wrappers routinely spawn grandchildren that otherwise
/// survive a direct-child kill and keep running in the background.
pub fn isolate_group(cmd: &mut Command) {
    #[cfg(unix)]
    cmd.process_group(0);
    #[cfg(not(unix))]
    let _ = cmd;
}

#[cfg(unix)]
fn signal_group(pid: u32, sig: i32) {
    // The child leads its own group, so a negative pid targets all members.
    // Stale or already-reaped groups yield ESRCH, which is safe to ignore.
    unsafe {
        libc::kill(-(pid as libc::pid_t), sig);
    }
}

/// Kills the child and all of its descendants.
pub async fn kill_tree(child: &mut tokio::process::Child) {
    #[cfg(unix)]
    if let Some(pid) = child.id() {
        signal_group(pid, libc::SIGKILL);
    }
    let _ = child.kill().await;
}

/// Synchronous kill for `Drop` contexts that cannot await reaping.
pub fn kill_tree_sync(child: &mut tokio::process::Child) {
    #[cfg(unix)]
    if let Some(pid) = child.id() {
        signal_group(pid, libc::SIGKILL);
    }
    let _ = child.start_kill();
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn kill_tree_terminates_the_whole_group() {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg("sleep 30 & wait");
        isolate_group(&mut cmd);
        let mut child = cmd.spawn().expect("spawn test shell");
        let pid = child.id().expect("child pid");

        kill_tree(&mut child).await;
        child.wait().await.expect("reap child");

        // Every group member is gone once probing the group reports ESRCH.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while std::time::Instant::now() < deadline {
            if unsafe { libc::kill(-(pid as libc::pid_t), 0) } == -1 {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        panic!("process group {pid} still has living members");
    }
}
