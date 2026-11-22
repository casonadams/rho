//! Process-group isolation and RAII lifecycle guards so kills reach the entire command tree.

mod guard;
mod shutdown;
mod streaming;
#[cfg(test)]
mod tests;

pub use guard::ProcessTreeGuard;
pub use shutdown::{install_panic_hook, shutdown_children};
pub use streaming::{StreamingCommand, configure_shell_command};

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex, MutexGuard, PoisonError};
use tokio::process::Command;

static TRACKED: LazyLock<Registry> = LazyLock::new(Registry::default);

/// How long a tracked process lives: tool jobs end with the turn that started them, while servers
/// (language servers, MCP) serve the whole session.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scope {
    Turn,
    Session,
}

/// Places the child in its own session and process group so one group kill
/// reaches every descendant. `sh -c` wrappers routinely spawn grandchildren
/// that otherwise survive a direct-child kill. Dropping the controlling
/// terminal makes `/dev/tty` reads fail instead of stopping the group.
pub fn isolate_group(cmd: &mut Command) {
    #[cfg(unix)]
    // SAFETY: setsid is async-signal-safe and the closure touches no other state.
    unsafe {
        cmd.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
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
        libc::kill(pid as libc::pid_t, sig);
    }
}

/// On macOS a child forked during a group SIGKILL can miss it (measured: ~6% of fork loops); Linux
/// delays such signals until after fork. Re-signal until only zombies (EPERM) or none (ESRCH) remain.
#[cfg(target_os = "macos")]
fn kill_forked_stragglers(pid: u32) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(250);
    while std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(1));
        if unsafe { libc::kill(-(pid as libc::pid_t), libc::SIGKILL) } != 0 {
            return;
        }
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
    #[cfg(target_os = "macos")]
    kill_forked_stragglers(pid);
    #[cfg(windows)]
    kill_windows_tree(pid);
    #[cfg(not(any(unix, windows)))]
    let _ = pid;
}

/// Like [`kill_group_by_pid`] but leaves the macOS straggler sweep to a background thread, for
/// callers on async workers that must not sleep. Not for exit paths, which must finish the sweep.
pub(crate) fn kill_group_detached(pid: u32) {
    #[cfg(target_os = "macos")]
    if pid > 1 {
        signal_group(pid, libc::SIGKILL);
        std::thread::spawn(move || kill_forked_stragglers(pid));
    }
    #[cfg(not(target_os = "macos"))]
    kill_group_by_pid(pid);
}

/// Registers a child PID to be terminated if the harness shuts down unexpectedly.
pub fn track_pid(pid: u32) {
    track(pid, Scope::Turn);
}

/// Child PIDs to terminate when the harness shuts down or a turn is cancelled. The process-wide
/// instance is [`TRACKED`]; tests that kill by scope use their own so they cannot reap other tests'
/// children.
#[derive(Default)]
struct Registry {
    pids: Mutex<HashMap<u32, Scope>>,
}

impl Registry {
    /// The panic hook kills tracked processes after a panic that may have poisoned the lock, so
    /// poisoning is ignored: the map is always left consistent by its short critical sections.
    fn lock(&self) -> MutexGuard<'_, HashMap<u32, Scope>> {
        self.pids.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn track(&self, pid: u32, scope: Scope) {
        if pid > 1 {
            self.lock().insert(pid, scope);
        }
    }

    fn kill_all(&self) {
        let pids: Vec<u32> = self.lock().drain().map(|(pid, _)| pid).collect();
        pids.into_iter().for_each(kill_group_by_pid);
    }

    fn kill_turn(&self) {
        let mut turn_pids = Vec::new();
        self.lock().retain(|pid, scope| {
            let turn = *scope == Scope::Turn;
            if turn {
                turn_pids.push(*pid);
            }
            !turn
        });
        turn_pids.into_iter().for_each(kill_group_detached);
    }
}

pub(crate) fn track(pid: u32, scope: Scope) {
    TRACKED.track(pid, scope);
}

/// Unregisters a child PID once it has been reaped or cleaned up.
pub fn untrack_pid(pid: u32) {
    TRACKED.lock().remove(&pid);
}

/// Synchronously kills all currently tracked process groups, including session-long servers.
pub fn kill_all_tracked_processes() {
    TRACKED.kill_all();
}

/// Kills the tracked tool jobs of the current turn (for example when it is cancelled) and leaves
/// session-long servers running.
pub fn kill_turn_processes() {
    TRACKED.kill_turn();
}

#[cfg(test)]
pub fn is_pid_tracked(pid: u32) -> bool {
    TRACKED.lock().contains_key(&pid)
}

#[cfg(all(test, unix))]
pub async fn wait_group_dead(pid: u32) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(2500);
    while std::time::Instant::now() < deadline {
        if unsafe { libc::kill(-(pid as libc::pid_t), 0) } == -1 {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("process group {pid} still has living members");
}
