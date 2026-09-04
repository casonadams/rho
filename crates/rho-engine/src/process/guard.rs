use super::{kill_group_by_pid, track_pid, untrack_pid};
use tokio::process::Child;

/// RAII guard wrapping a child process to guarantee whole-tree termination on drop.
pub struct ProcessTreeGuard {
    child: Option<Child>,
    pid: Option<u32>,
}

impl ProcessTreeGuard {
    pub fn new(child: Child) -> Self {
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

    pub fn child_mut(&mut self) -> Option<&mut Child> {
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
        let _ = self.kill_and_wait().await;
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

    pub fn disarm(mut self) -> Option<Child> {
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
