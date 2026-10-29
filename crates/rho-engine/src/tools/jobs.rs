use crate::tools::types::ToolResult;
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
}

#[derive(Clone, Default)]
pub struct JobManager {
    jobs: Arc<Mutex<HashMap<String, BackgroundJobInfo>>>,
}

static GLOBAL_JOB_MANAGER: std::sync::LazyLock<JobManager> = std::sync::LazyLock::new(JobManager::default);

impl JobManager {
    pub fn global() -> &'static Self {
        &GLOBAL_JOB_MANAGER
    }

    pub fn new() -> Self {
        Self {
            jobs: Arc::new(Mutex::new(HashMap::new())),
        }
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
            },
        );
    }

    pub fn update_status(&self, id: &str, status: JobStatus) {
        let mut guard = self.jobs.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(job) = guard.get_mut(id) {
            job.status = status;
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
}

pub fn format_jobs_list(jobs: &[BackgroundJobInfo]) -> ToolResult {
    if jobs.is_empty() {
        return ToolResult::success("No background jobs found.");
    }

    let mut out = String::from("Background Jobs:\n");
    for job in jobs {
        let elapsed = job.started_at.elapsed().as_secs();
        match &job.status {
            JobStatus::Running => {
                out.push_str(&format!("- [{}] RUNNING ({}s): {}\n", job.id, elapsed, job.command));
            }
            JobStatus::Completed { exit_code, .. } => {
                out.push_str(&format!(
                    "- [{}] COMPLETED (exit: {}): {}\n",
                    job.id, exit_code, job.command
                ));
            }
            JobStatus::Failed { error } => {
                out.push_str(&format!("- [{}] FAILED ({}): {}\n", job.id, error, job.command));
            }
        }
    }
    ToolResult::success(out.trim_end())
}

pub struct JobsTool;

impl Default for JobsTool {
    fn default() -> Self {
        Self::new()
    }
}

impl JobsTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl crate::tools::engine_tool::EngineTool for JobsTool {
    fn name(&self) -> &str {
        "jobs"
    }

    fn description(&self) -> &str {
        "List and inspect background jobs and processes initiated by bash."
    }

    fn parameters(&self) -> serde_json::Value {
        crate::tools::types::generated_schema::<rho_harness_core::args::JobsArgs>()
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult, rho_harness_core::error::AppError> {
        let parsed: rho_harness_core::args::JobsArgs = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::error(format!("failed to parse jobs arguments: {e}"))),
        };

        if let Some(id) = parsed.job_id {
            if let Some(job) = JobManager::global().get(&id) {
                let status_desc = match &job.status {
                    JobStatus::Running => format!("RUNNING ({}s)", job.started_at.elapsed().as_secs()),
                    JobStatus::Completed { exit_code, output } => {
                        format!("COMPLETED (exit: {exit_code})\n\nOutput:\n{output}")
                    }
                    JobStatus::Failed { error } => format!("FAILED: {error}"),
                };
                return Ok(ToolResult::success(format!(
                    "Job: {}\nCommand: {}\nStatus: {}",
                    job.id, job.command, status_desc
                )));
            }
            return Ok(ToolResult::error(format!("Job '{id}' not found.")));
        }

        let jobs = JobManager::global().list();
        Ok(format_jobs_list(&jobs))
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
    }

    #[tokio::test]
    async fn test_jobs_tool_execute() {
        let tool = JobsTool::new();
        assert_eq!(tool.name(), "jobs");
        assert!(!tool.description().is_empty());
        let _ = tool.parameters();
        assert!(!tool.execute(serde_json::json!({})).await.unwrap().is_error);
        assert!(tool.execute(serde_json::json!({"job_id": 1})).await.unwrap().is_error);
        assert!(tool.execute(serde_json::json!({"job_id": "x"})).await.unwrap().is_error);

        JobManager::global().register("gj-1".into(), "cmd".into());
        let check = || tool.execute(serde_json::json!({"job_id": "gj-1"}));
        assert!(check().await.unwrap().content.contains("RUNNING"));
        JobManager::global().update_status("gj-1", done(0, "ok"));
        assert!(check().await.unwrap().content.contains("COMPLETED"));
        JobManager::global().update_status("gj-1", JobStatus::Failed { error: "e".into() });
        assert!(check().await.unwrap().content.contains("FAILED"));
    }

    #[test]
    fn test_format_jobs_list() {
        assert!(format_jobs_list(&[]).content.contains("No background jobs"));
        let make_job = |id: &str, status| BackgroundJobInfo {
            id: id.into(),
            command: "c".into(),
            started_at: std::time::Instant::now(),
            status,
        };
        let jobs = vec![
            make_job("j1", JobStatus::Running),
            make_job("j2", done(0, "")),
            make_job("j3", JobStatus::Failed { error: "e".into() }),
        ];
        let res = format_jobs_list(&jobs);
        assert!(res.content.contains("RUNNING") && res.content.contains("COMPLETED") && res.content.contains("FAILED"));
    }
}
