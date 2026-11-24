use rho_harness_core::a2a::TaskState;
use serde_json::{Value, json};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

const MAX_OUTPUT_JSON_CHARS: usize = 6_000;
const MAX_LISTED_TASKS: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StepStatus {
    Done,
    Skipped,
    Failed,
}

impl StepStatus {
    fn symbol(self) -> &'static str {
        match self {
            Self::Done => "✓",
            Self::Skipped => "·",
            Self::Failed => "✗",
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Done => "done",
            Self::Skipped => "skipped",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StepRecord {
    pub path: String,
    pub status: StepStatus,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentTask {
    pub path: String,
    pub id: String,
    pub state: TaskState,
}

pub(crate) struct RunLog {
    id: String,
    path: PathBuf,
    file: Mutex<Option<std::fs::File>>,
    records: Mutex<Vec<StepRecord>>,
    tasks: Mutex<Vec<AgentTask>>,
    files: Mutex<super::files::AgentFileLog>,
    warnings: Mutex<Vec<String>>,
}

impl RunLog {
    pub(crate) fn create(base_dir: &Path, workflow: &str) -> Result<Self, String> {
        let dir = base_dir.join(".rho/workflows");
        std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        let ignore = dir.join(".gitignore");
        if !ignore.exists() {
            std::fs::write(&ignore, "*\n").map_err(|e| format!("cannot create {}: {e}", ignore.display()))?;
        }
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let id = uuid::Uuid::new_v4().simple().to_string();
        let run_id = format!("{stamp}-{}", &id[..8]);
        let path = dir.join(format!("{workflow}-{run_id}.jsonl"));
        let file = std::fs::File::create(&path).map_err(|e| format!("cannot create {}: {e}", path.display()))?;
        Ok(Self {
            id: run_id,
            path,
            file: Mutex::new(Some(file)),
            records: Mutex::new(Vec::new()),
            tasks: Mutex::new(Vec::new()),
            files: Mutex::default(),
            warnings: Mutex::new(Vec::new()),
        })
    }

    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    /// Records an agent step's result so a later run can resume from it.
    pub(crate) fn journal(&self, step: &str, key: &str, value: &Value, detail: &str) {
        let line = json!({ "step": step, "key": key, "value": value, "detail": detail });
        let path = super::resume::journal_path(&self.path);
        let written = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .and_then(|mut file| writeln!(file, "{line}"));
        if let Err(e) = written {
            self.warn(format!("cannot journal {step} for resume: {e}"));
        }
    }

    #[cfg(test)]
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn event(&self, kind: &str, fields: Value) {
        let mut line = json!({ "ts": chrono::Utc::now().to_rfc3339(), "event": kind });
        if let (Some(target), Value::Object(extra)) = (line.as_object_mut(), fields) {
            target.extend(extra);
        }
        let mut guard = self.file.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(file) = guard.as_mut() else {
            return;
        };
        if let Err(e) = writeln!(file, "{line}") {
            *guard = None;
            self.warn(format!("run log disabled after write failure: {e}"));
        }
    }

    pub(crate) fn warn(&self, message: String) {
        self.warnings
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(message);
    }

    pub(crate) fn record(&self, path: &str, status: StepStatus, detail: impl Into<String>) {
        let detail = detail.into();
        self.event(
            "step_finished",
            json!({ "step": path, "status": status.as_str(), "detail": detail }),
        );
        self.records
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(StepRecord {
                path: path.to_string(),
                status,
                detail,
            });
    }

    pub(crate) fn records(&self) -> Vec<StepRecord> {
        self.records.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    pub(crate) fn task(&self, path: &str, id: &str, state: TaskState) {
        self.tasks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(AgentTask {
                path: path.to_string(),
                id: id.to_string(),
                state,
            });
    }

    pub(crate) fn agent_files(
        &self,
        step: &str,
        round: &crate::tools::subagent::round::Round,
        ran: std::ops::Range<std::time::Instant>,
    ) {
        self.files
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .add(step, round, ran);
    }

    /// Saved agent tasks, unfinished ones first, then the most recent completed ones.
    fn task_section(&self) -> String {
        let tasks = self.tasks.lock().unwrap_or_else(PoisonError::into_inner).clone();
        if tasks.is_empty() {
            return String::new();
        }
        let (completed, unfinished): (Vec<AgentTask>, Vec<AgentTask>) =
            tasks.iter().cloned().partition(|t| t.state == TaskState::Completed);
        let listed: Vec<&AgentTask> = unfinished
            .iter()
            .chain(completed.iter().rev())
            .take(MAX_LISTED_TASKS)
            .collect();
        let mut out = String::from(
            "\nAgent tasks (continue one with the subagent tool's `task_id` to review, fix, or finish its work):\n",
        );
        for task in &listed {
            out.push_str(&format!(
                "  {} — task `{}` · {}\n",
                task.path,
                task.id,
                task.state.label()
            ));
        }
        if tasks.len() > listed.len() {
            out.push_str(&format!("  ... {} more in the run log\n", tasks.len() - listed.len()));
        }
        out
    }

    pub(crate) fn report(&self, headline: &str, outputs: Option<&Value>) -> String {
        let mut out = format!("{headline}\n\nSteps:\n");
        for record in self.records() {
            let detail = if record.detail.is_empty() {
                String::new()
            } else {
                format!(" — {}", record.detail)
            };
            out.push_str(&format!("  {} {}{detail}\n", record.status.symbol(), record.path));
        }
        if let Some(outputs) = outputs.filter(|o| o.as_object().is_some_and(|m| !m.is_empty())) {
            out.push_str(&format!("\nOutputs:\n```json\n{}\n```\n", clamp_json(outputs)));
        }
        out.push_str(&self.files.lock().unwrap_or_else(PoisonError::into_inner).section());
        out.push_str(&self.task_section());
        for warning in self.warnings.lock().unwrap_or_else(PoisonError::into_inner).iter() {
            out.push_str(&format!("\nWarning: {warning}\n"));
        }
        out.push_str(&format!("\nRun log: {}", self.path.display()));
        out
    }
}

fn clamp_json(value: &Value) -> String {
    let text = serde_json::to_string_pretty(value).unwrap_or_default();
    if text.chars().count() <= MAX_OUTPUT_JSON_CHARS {
        return text;
    }
    let kept: String = text.chars().take(MAX_OUTPUT_JSON_CHARS).collect();
    format!("{kept}\n... (truncated; see the run log)")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_jsonl_events_and_renders_a_step_table() {
        let dir = tempfile::tempdir().unwrap();
        let log = RunLog::create(dir.path(), "refine").unwrap();
        log.event("run_started", json!({ "workflow": "refine" }));
        log.record("refine › round 1 › scan", StepStatus::Done, "3 items");
        log.record("refine › round 1 › implement", StepStatus::Skipped, "");

        let text = std::fs::read_to_string(log.path()).unwrap();
        let events: Vec<Value> = text.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(events[0]["event"], "run_started");
        assert_eq!(events[1]["step"], "refine › round 1 › scan");
        assert_eq!(events[2]["status"], "skipped");

        let report = log.report("Workflow `refine` finished.", Some(&json!({ "remaining": [] })));
        assert!(report.contains("  ✓ refine › round 1 › scan — 3 items\n"));
        assert!(report.contains("  · refine › round 1 › implement\n"));
        assert!(report.contains("\"remaining\": []"));
        assert!(report.ends_with(&format!("Run log: {}", log.path().display())));
    }
}
