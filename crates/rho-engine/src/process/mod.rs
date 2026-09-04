//! Process-group isolation and RAII lifecycle guards so kills reach the entire command tree.

mod guard;
#[cfg(test)]
mod tests;

pub use guard::ProcessTreeGuard;

use std::collections::HashSet;
use std::sync::{LazyLock, Mutex};
use tokio::process::Command;

static TRACKED_PIDS: LazyLock<Mutex<HashSet<u32>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

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
    if pid <= 1 {
        return;
    }
    // The child leads its own group, so a negative pid targets all members.
    // Stale or already-reaped groups yield ESRCH, which is safe to ignore.
    unsafe {
        libc::kill(-(pid as libc::pid_t), sig);
    }
}

#[cfg(windows)]
fn kill_windows_tree(pid: u32) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    let _ = std::process::Command::new("taskkill")
        .args(["/F", "/T", "/PID", &pid.to_string()])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
}

pub(crate) fn kill_group_by_pid(pid: u32) {
    if pid <= 1 {
        return;
    }
    #[cfg(unix)]
    signal_group(pid, libc::SIGKILL);
    #[cfg(windows)]
    kill_windows_tree(pid);
    #[cfg(not(any(unix, windows)))]
    let _ = pid;
}

/// Registers a child PID to be terminated if the harness shuts down unexpectedly.
pub fn track_pid(pid: u32) {
    if pid > 1
        && let Ok(mut set) = TRACKED_PIDS.lock()
    {
        set.insert(pid);
    }
}

/// Unregisters a child PID once it has been reaped or cleaned up.
pub fn untrack_pid(pid: u32) {
    if let Ok(mut set) = TRACKED_PIDS.lock() {
        set.remove(&pid);
    }
}

/// Synchronously kills all currently tracked process groups.
pub fn kill_all_tracked_processes() {
    if let Ok(mut set) = TRACKED_PIDS.lock() {
        for pid in set.drain() {
            kill_group_by_pid(pid);
        }
    }
}

#[cfg(test)]
pub fn tracked_pid_count() -> usize {
    TRACKED_PIDS.lock().map(|s| s.len()).unwrap_or(0)
}

#[cfg(test)]
pub fn is_pid_tracked(pid: u32) -> bool {
    TRACKED_PIDS.lock().map(|s| s.contains(&pid)).unwrap_or(false)
}

#[cfg(all(test, unix))]
pub async fn wait_group_dead(pid: u32) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        if unsafe { libc::kill(-(pid as libc::pid_t), 0) } == -1 {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("process group {pid} still has living members");
}

/// Kills the child and all of its descendants.
pub async fn kill_tree(child: &mut tokio::process::Child) {
    if let Some(pid) = child.id() {
        kill_group_by_pid(pid);
    }
    let _ = child.kill().await;
}

/// Synchronous kill for `Drop` contexts that cannot await reaping.
pub fn kill_tree_sync(child: &mut tokio::process::Child) {
    if let Some(pid) = child.id() {
        kill_group_by_pid(pid);
    }
    let _ = child.start_kill();
}
