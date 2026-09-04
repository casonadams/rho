//! Process-group isolation and RAII lifecycle guards so kills reach the entire command tree.

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

fn kill_group_by_pid(pid: u32) {
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

/// RAII guard wrapping a child process to guarantee whole-tree termination on drop.
pub struct ProcessTreeGuard {
    child: Option<tokio::process::Child>,
    pid: Option<u32>,
}

impl ProcessTreeGuard {
    pub fn new(child: tokio::process::Child) -> Self {
        let pid = child.id();
        if let Some(pid) = pid {
            track_pid(pid);
        }
        Self {
            child: Some(child),
            pid,
        }
    }

    pub fn id(&self) -> Option<u32> {
        self.pid
    }

    pub fn child_mut(&mut self) -> Option<&mut tokio::process::Child> {
        self.child.as_mut()
    }

    pub async fn wait(&mut self) -> std::io::Result<std::process::ExitStatus> {
        let child = self
            .child
            .as_mut()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "child already reaped"))?;
        let res = child.wait().await;
        if res.is_ok() {
            if let Some(pid) = self.pid.take() {
                untrack_pid(pid);
            }
            self.child = None;
        }
        res
    }

    pub async fn kill(&mut self) {
        if let Some(mut child) = self.child.take() {
            if let Some(pid) = self.pid.take() {
                untrack_pid(pid);
                kill_group_by_pid(pid);
            }
            let _ = child.kill().await;
        }
    }

    pub async fn kill_and_wait(&mut self) -> std::io::Result<std::process::ExitStatus> {
        if let Some(mut child) = self.child.take() {
            if let Some(pid) = self.pid.take() {
                untrack_pid(pid);
                kill_group_by_pid(pid);
            }
            let _ = child.kill().await;
            child.wait().await
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "child already reaped",
            ))
        }
    }

    pub fn disarm(mut self) -> Option<tokio::process::Child> {
        if let Some(pid) = self.pid.take() {
            untrack_pid(pid);
        }
        self.child.take()
    }
}

impl Drop for ProcessTreeGuard {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            if let Some(pid) = self.pid.take() {
                untrack_pid(pid);
                kill_group_by_pid(pid);
            }
            let _ = child.start_kill();
        }
    }
}

#[cfg(test)]
mod tests;
