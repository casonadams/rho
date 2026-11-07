use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

pub const MAX_FINISHED_JOBS: usize = 32;

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
    pub log_path: Option<PathBuf>,
}

#[derive(Default)]
struct Registry {
    jobs: HashMap<String, BackgroundJobInfo>,
    finished: VecDeque<String>,
}

#[derive(Clone, Default)]
pub struct JobManager {
    registry: Arc<Mutex<Registry>>,
    notify: Arc<tokio::sync::Notify>,
}

static GLOBAL_JOB_MANAGER: std::sync::LazyLock<JobManager> = std::sync::LazyLock::new(JobManager::default);

fn evict_finished(registry: &mut Registry) {
    while registry.finished.len() > MAX_FINISHED_JOBS {
        let Some(id) = registry.finished.pop_front() else {
            break;
        };
        if let Some(job) = registry.jobs.remove(&id)
            && let Some(path) = job.log_path
        {
            let _ = std::fs::remove_file(path);
        }
    }
}

impl JobManager {
    pub fn global() -> &'static Self {
        &GLOBAL_JOB_MANAGER
    }

    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> MutexGuard<'_, Registry> {
        self.registry.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn register(&self, id: String, command: String) {
        self.lock().jobs.insert(
            id.clone(),
            BackgroundJobInfo {
                id,
                command,
                started_at: Instant::now(),
                status: JobStatus::Running,
                consumed: false,
                log_path: None,
            },
        );
    }

    pub fn attach_log(&self, id: &str, path: PathBuf) {
        if let Some(job) = self.lock().jobs.get_mut(id) {
            job.log_path = Some(path);
        }
    }

    pub fn update_status(&self, id: &str, status: JobStatus) {
        {
            let mut registry = self.lock();
            let Registry { jobs, finished } = &mut *registry;
            if let Some(job) = jobs.get_mut(id) {
                let finishing = job.status == JobStatus::Running && status != JobStatus::Running;
                job.status = status;
                job.consumed = false;
                if finishing {
                    finished.push_back(id.to_string());
                }
            }
            evict_finished(&mut registry);
        }
        self.notify.notify_waiters();
    }

    pub fn mark_consumed(&self, id: &str) {
        if let Some(job) = self.lock().jobs.get_mut(id) {
            job.consumed = true;
        }
    }

    pub fn get(&self, id: &str) -> Option<BackgroundJobInfo> {
        self.lock().jobs.get(id).cloned()
    }

    pub fn list(&self) -> Vec<BackgroundJobInfo> {
        let mut res: Vec<BackgroundJobInfo> = self.lock().jobs.values().cloned().collect();
        res.sort_by_key(|j| j.started_at);
        res
    }

    pub fn running_count(&self) -> usize {
        self.lock()
            .jobs
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

    #[test]
    fn finished_jobs_are_bounded_and_evict_their_logs() {
        let mgr = JobManager::new();
        let dir = tempfile::tempdir().unwrap();
        let first_log = dir.path().join("first.log");
        std::fs::write(&first_log, "output").unwrap();
        mgr.register("job-0".into(), "cmd".into());
        mgr.attach_log("job-0", first_log.clone());
        mgr.update_status("job-0", done(0, "ok"));
        for i in 1..=MAX_FINISHED_JOBS {
            let id = format!("job-{i}");
            mgr.register(id.clone(), "cmd".into());
            mgr.update_status(&id, done(0, "ok"));
        }
        assert!(mgr.get("job-0").is_none());
        assert!(!first_log.exists());
        assert_eq!(mgr.list().len(), MAX_FINISHED_JOBS);
    }
}
