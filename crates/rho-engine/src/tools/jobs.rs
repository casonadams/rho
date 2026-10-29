use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobStatus {
    Running,
    Completed { exit_code: i32, output: String },
    Failed { error: String },
}

#[derive(Debug, Clone)]
pub struct BackgroundJobInfo {
    pub id: String,
    pub command: String,
    pub started_at: Instant,
    pub status: JobStatus,
    pub consumed: bool,
}

#[derive(Clone, Default)]
pub struct JobManager {
    jobs: Arc<Mutex<HashMap<String, BackgroundJobInfo>>>,
    notify: Arc<tokio::sync::Notify>,
}

static GLOBAL_JOB_MANAGER: std::sync::LazyLock<JobManager> = std::sync::LazyLock::new(JobManager::default);

impl JobManager {
    pub fn global() -> &'static Self {
        &GLOBAL_JOB_MANAGER
    }

    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self, id: String, command: String) {
        let mut guard = self.jobs.lock().unwrap_or_else(|p| p.into_inner());
        guard.insert(
            id.clone(),
            BackgroundJobInfo {
                id,
                command,
                started_at: Instant::now(),
                status: JobStatus::Running,
                consumed: false,
            },
        );
    }

    fn mutate_job<F>(&self, id: &str, f: F)
    where
        F: FnOnce(&mut BackgroundJobInfo),
    {
        let mut guard = self.jobs.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(job) = guard.get_mut(id) {
            f(job);
        }
    }

    pub fn update_status(&self, id: &str, status: JobStatus) {
        self.mutate_job(id, |job| {
            job.status = status;
            job.consumed = false;
        });
        self.notify.notify_waiters();
    }

    pub fn mark_consumed(&self, id: &str) {
        self.mutate_job(id, |job| job.consumed = true);
    }

    pub fn get(&self, id: &str) -> Option<BackgroundJobInfo> {
        let guard = self.jobs.lock().unwrap_or_else(|p| p.into_inner());
        guard.get(id).cloned()
    }

    pub fn list(&self) -> Vec<BackgroundJobInfo> {
        let mut res = Vec::new();
        if let Ok(guard) = self.jobs.lock() {
            res.extend(guard.values().cloned());
            res.sort_by_key(|j| j.started_at);
        }
        res
    }

    pub fn running_count(&self) -> usize {
        let Ok(guard) = self.jobs.lock() else {
            return 0;
        };
        guard
            .values()
            .filter(|j| matches!(j.status, JobStatus::Running))
            .count()
    }

    pub fn has_running(&self) -> bool {
        self.running_count() > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn done(exit_code: i32, output: &str) -> JobStatus {
        JobStatus::Completed {
            exit_code,
            output: output.into(),
        }
    }

    #[test]
    fn test_job_manager_lifecycle() {
        let mgr = JobManager::new();
        mgr.register("j1".into(), "echo 1".into());
        assert_eq!(mgr.get("j1").unwrap().status, JobStatus::Running);
        assert!(mgr.has_running());
        assert_eq!(mgr.running_count(), 1);

        mgr.update_status("j1", done(0, "ok"));
        assert_eq!(mgr.get("j1").unwrap().status, done(0, "ok"));
        assert!(!mgr.has_running());
        assert_eq!(mgr.running_count(), 0);

        mgr.mark_consumed("j1");
        assert!(mgr.get("j1").unwrap().consumed);
        assert!(!mgr.list().is_empty());
    }
}
