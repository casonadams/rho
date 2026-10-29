use crate::tools::types::ToolResult;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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

#[derive(Clone)]
pub struct JobManager {
    jobs: Arc<Mutex<HashMap<String, BackgroundJobInfo>>>,
    notify: Arc<tokio::sync::Notify>,
}

impl Default for JobManager {
    fn default() -> Self {
        Self {
            jobs: Arc::new(Mutex::new(HashMap::new())),
            notify: Arc::new(tokio::sync::Notify::new()),
        }
    }
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

    pub fn update_status(&self, id: &str, status: JobStatus) {
        let mut guard = self.jobs.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(job) = guard.get_mut(id) {
            job.status = status;
            job.consumed = false;
        }
        self.notify.notify_waiters();
    }

    pub fn mark_consumed(&self, id: &str) {
        let mut guard = self.jobs.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(job) = guard.get_mut(id) {
            job.consumed = true;
        }
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

    pub async fn wait_any(&self, timeout_dur: Duration) -> Result<BackgroundJobInfo, String> {
        let deadline = tokio::time::Instant::now() + timeout_dur;
        loop {
            let notified = self.notify.notified();
            tokio::pin!(notified);

            let (unconsumed, has_running) = {
                let Ok(guard) = self.jobs.lock() else {
                    return Err("Failed to acquire jobs lock".to_string());
                };
                let unconsumed = guard
                    .values()
                    .filter(|j| !j.consumed && !matches!(j.status, JobStatus::Running))
                    .cloned()
                    .min_by_key(|j| j.started_at);
                let has_running = guard.values().any(|j| matches!(j.status, JobStatus::Running));
                (unconsumed, has_running)
            };

            if let Some(job) = unconsumed {
                self.mark_consumed(&job.id);
                return Ok(job);
            }

            if !has_running {
                return Err("No running background jobs to wait for.".to_string());
            }

            if tokio::time::timeout_at(deadline, notified).await.is_err() {
                return Err("Timed out waiting for background jobs.".to_string());
            }
        }
    }
}

fn format_wait_result(job: &BackgroundJobInfo) -> ToolResult {
    match &job.status {
        JobStatus::Running => ToolResult::success(format!(
            "Job '{}' is still running after timeout ({}s).\nCommand: {}",
            job.id,
            job.started_at.elapsed().as_secs(),
            job.command
        )),
        JobStatus::Completed { exit_code, output } => ToolResult::success(format!(
            "Background job '{}' completed with exit code {exit_code}.\nCommand: {}\n\nOutput:\n{output}",
            job.id, job.command
        )),
        JobStatus::Failed { error } => ToolResult::error(format!(
            "Background job '{}' failed: {error}.\nCommand: {}",
            job.id, job.command
        )),
    }
}

pub struct WaitTool;

impl Default for WaitTool {
    fn default() -> Self {
        Self::new()
    }
}

impl WaitTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl crate::tools::engine_tool::EngineTool for WaitTool {
    fn name(&self) -> &str {
        "wait"
    }

    fn description(&self) -> &str {
        "Wait only when blocked with nothing else to do. Returns on the first background result. Results auto-deliver; NEVER poll with sleep while work remains."
    }

    fn parameters(&self) -> serde_json::Value {
        crate::tools::types::generated_schema::<rho_harness_core::args::WaitArgs>()
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult, rho_harness_core::error::AppError> {
        let parsed: rho_harness_core::args::WaitArgs = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::error(format!("failed to parse wait arguments: {e}"))),
        };

        let timeout = Duration::from_secs(parsed.timeout.unwrap_or(300));
        match JobManager::global().wait_any(timeout).await {
            Ok(job) => Ok(format_wait_result(&job)),
            Err(err) => Ok(ToolResult::success(err)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::engine_tool::EngineTool;

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
        mgr.update_status("j1", done(0, "ok"));
        assert_eq!(mgr.get("j1").unwrap().status, done(0, "ok"));
        assert!(!mgr.list().is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn test_job_manager_wait_any() {
        let mgr = JobManager::new();
        assert!(mgr.wait_any(Duration::from_secs(1)).await.is_err());

        mgr.register("a1".into(), "test cmd".into());
        let mgr_clone = mgr.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            mgr_clone.update_status("a1", done(0, "finished"));
        });

        let job = mgr.wait_any(Duration::from_secs(5)).await.unwrap();
        assert_eq!(job.id, "a1");
        assert_eq!(job.status, done(0, "finished"));
    }

    #[tokio::test]
    async fn test_wait_tool_execute() {
        let tool = WaitTool::new();
        assert_eq!(tool.name(), "wait");
        assert!(!tool.description().is_empty());
        let _ = tool.parameters();

        // No jobs running
        let res = tool.execute(serde_json::json!({})).await.unwrap();
        assert!(res.content.contains("No running background jobs"));

        // Register and complete a job
        JobManager::global().register("w-job-1".into(), "cargo check".into());
        JobManager::global().update_status("w-job-1", done(0, "check passed"));

        let res = tool.execute(serde_json::json!({})).await.unwrap();
        assert!(!res.is_error);
        assert!(res.content.contains("check passed"));
        assert!(res.content.contains("w-job-1"));
    }
}
