use crate::engine::runner::turn::types::CancellationSignal;
use crate::mailbox::{AgentId, NoticeTarget};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;
use tokio::sync::watch;

const MAX_FINISHED_JOBS: usize = 32;

crate::id::string_id!(
    /// Names one background job; a subagent's job is named by its task id.
    JobId
);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobStatus {
    Running,
    Completed { exit_code: i32, output: String },
    Failed { error: String },
}

#[derive(Debug, Clone)]
pub struct BackgroundJobInfo {
    pub id: JobId,
    /// The agent whose turns wait for this job and receive its notice.
    pub owner: AgentId,
    pub command: String,
    pub started_at: Instant,
    pub status: JobStatus,
    pub log_path: Option<PathBuf>,
    pub cancel: CancellationSignal,
}

impl BackgroundJobInfo {
    fn runs_for(&self, owner: &AgentId) -> bool {
        &self.owner == owner && self.status == JobStatus::Running
    }
}

#[derive(Default)]
struct Registry {
    jobs: HashMap<JobId, BackgroundJobInfo>,
    finished: VecDeque<JobId>,
}

#[derive(Clone)]
pub struct JobManager {
    registry: Arc<Mutex<Registry>>,
    /// Counts jobs that stopped running, so waiters wake when one ends without sending a message.
    ended: Arc<watch::Sender<u64>>,
}

impl Default for JobManager {
    fn default() -> Self {
        Self {
            registry: Arc::default(),
            ended: Arc::new(watch::Sender::new(0)),
        }
    }
}

/// A running job's handle. Finishing it records the outcome; dropping it unfinished (a panic or an
/// aborted task) marks the job failed so no turn waits on it forever.
pub struct JobCompletion {
    jobs: JobManager,
    id: JobId,
    /// Where the owner's notice goes.
    owner: NoticeTarget,
    cancel: CancellationSignal,
    finished: bool,
}

impl JobCompletion {
    pub(crate) fn id(&self) -> &JobId {
        &self.id
    }

    /// Fires when the job must stop: its turn was interrupted or its session ended.
    pub(crate) fn cancel(&self) -> &CancellationSignal {
        &self.cancel
    }

    /// Records where the job's full output was saved.
    pub(crate) fn attach_log(&self, path: PathBuf) {
        self.jobs.attach_log(&self.id, path);
    }

    /// Records `status`; `notice` reaches the owner's mailbox unless the job was stopped. The notice
    /// is sent before the job stops counting as running, so a waiting turn cannot miss it.
    pub(crate) fn finish(mut self, status: JobStatus, notice: Option<String>) {
        self.finished = true;
        if let Some(notice) = notice.filter(|_| !self.cancel.is_cancelled())
            && let Err(error) = self.owner.send(notice)
        {
            tracing::warn!("delivering the notice for job {} failed: {error}", self.id);
        }
        self.record(status);
    }

    fn record(&self, status: JobStatus) {
        self.jobs.record(&self.id, status, |job| job.cancel.same(&self.cancel));
    }
}

impl Drop for JobCompletion {
    fn drop(&mut self) {
        if !self.finished {
            let error = "stopped unexpectedly before reporting a result".to_string();
            self.record(JobStatus::Failed { error });
        }
    }
}

fn evict_finished(registry: &mut Registry) {
    while registry.finished.len() > MAX_FINISHED_JOBS {
        let Some(id) = registry.finished.pop_front() else {
            break;
        };
        if let Some(job) = registry.jobs.remove(&id)
            && let Some(path) = job.log_path
            && let Err(error) = std::fs::remove_file(&path)
        {
            tracing::warn!(
                "removing the log of an evicted job, {}, failed: {error}",
                path.display()
            );
        }
    }
}

impl JobManager {
    fn lock(&self) -> MutexGuard<'_, Registry> {
        crate::locking::lock(&self.registry)
    }

    /// Changes each time one of these jobs stops running.
    pub(crate) fn watch_ended(&self) -> watch::Receiver<u64> {
        self.ended.subscribe()
    }

    /// Registers a running job for the agent `owner`. The returned signal fires when the job must stop.
    pub(crate) fn register(&self, owner: &AgentId, id: JobId, command: String) -> CancellationSignal {
        let cancel = CancellationSignal::default();
        let mut registry = self.lock();
        registry.finished.retain(|finished| *finished != id);
        registry.jobs.insert(
            id.clone(),
            BackgroundJobInfo {
                id,
                owner: owner.clone(),
                command,
                started_at: Instant::now(),
                status: JobStatus::Running,
                log_path: None,
                cancel: cancel.clone(),
            },
        );
        cancel
    }

    /// Registers a running job for the agent `owner` and returns the handle that must finish it; its
    /// notice goes to `owner`.
    pub(crate) fn start(&self, owner: NoticeTarget, id: JobId, command: String) -> JobCompletion {
        let cancel = self.register(owner.agent(), id.clone(), command);
        JobCompletion {
            jobs: self.clone(),
            id,
            owner,
            cancel,
            finished: false,
        }
    }

    fn attach_log(&self, id: &JobId, path: PathBuf) {
        if let Some(job) = self.lock().jobs.get_mut(id) {
            job.log_path = Some(path);
        }
    }

    #[cfg(test)]
    pub(crate) fn update_status(&self, id: &JobId, status: JobStatus) {
        self.record(id, status, |_| true);
    }

    /// Records `status` for job `id` if `is_run` accepts its current record, so a finished run never
    /// overwrites a later run registered under the same id.
    fn record(&self, id: &JobId, status: JobStatus, is_run: impl Fn(&BackgroundJobInfo) -> bool) {
        let mut registry = self.lock();
        let Registry { jobs, finished } = &mut *registry;
        let mut ended = false;
        if let Some(job) = jobs.get_mut(id).filter(|job| is_run(job)) {
            ended = job.status == JobStatus::Running && status != JobStatus::Running;
            job.status = status;
            if ended {
                finished.push_back(id.clone());
            }
        }
        evict_finished(&mut registry);
        drop(registry);
        if ended {
            self.ended.send_modify(|count| *count += 1);
        }
    }

    pub fn get(&self, id: &JobId) -> Option<BackgroundJobInfo> {
        self.lock().jobs.get(id).cloned()
    }

    /// `owner`'s running jobs, oldest first.
    pub fn running(&self, owner: &AgentId) -> Vec<BackgroundJobInfo> {
        let mut running: Vec<BackgroundJobInfo> = self
            .lock()
            .jobs
            .values()
            .filter(|j| j.runs_for(owner))
            .cloned()
            .collect();
        running.sort_by_key(|j| j.started_at);
        running
    }

    pub fn has_running(&self, owner: &AgentId) -> bool {
        self.lock().jobs.values().any(|j| j.runs_for(owner))
    }

    /// Signals every running job of `owner` to stop; each job records its own failure when it does.
    pub fn cancel_owner(&self, owner: &AgentId) {
        for job in self.running(owner) {
            job.cancel.cancel();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mailbox::{Kind, Mailboxes, Party};

    fn id(name: &str) -> AgentId {
        AgentId::from(name)
    }

    fn job(name: &str) -> JobId {
        JobId::from(name)
    }

    fn done(exit_code: i32, output: &str) -> JobStatus {
        JobStatus::Completed {
            exit_code,
            output: output.into(),
        }
    }

    #[test]
    fn test_job_manager_lifecycle() {
        let mgr = JobManager::default();
        mgr.register(&id("me"), job("j1"), "echo 1".into());
        assert_eq!(mgr.get(&job("j1")).unwrap().status, JobStatus::Running);
        assert!(mgr.has_running(&id("me")));
        assert_eq!(mgr.running(&id("me")).len(), 1);

        mgr.update_status(&job("j1"), done(0, "ok"));
        assert_eq!(mgr.get(&job("j1")).unwrap().status, done(0, "ok"));
        assert!(!mgr.has_running(&id("me")));
        assert!(mgr.running(&id("me")).is_empty());
    }

    #[tokio::test]
    async fn a_job_whose_task_dies_before_reporting_stops_blocking_its_session() {
        let mgr = JobManager::default();
        let completion = mgr.start(
            crate::mailbox::Mailboxes::default().notices_for(&id("panicking-owner")),
            job("p1"),
            "cmd".into(),
        );
        let task = tokio::spawn(async move {
            let _job = completion;
            panic!("task died");
        });
        assert!(task.await.unwrap_err().is_panic());

        assert!(!mgr.has_running(&id("panicking-owner")));
        assert!(matches!(
            mgr.get(&job("p1")).unwrap().status,
            JobStatus::Failed { error } if error.contains("stopped unexpectedly")
        ));
    }

    #[test]
    fn a_finished_jobs_notice_reaches_its_owners_mailbox_unless_the_job_was_told_to_stop() {
        let boxes = Mailboxes::default();
        let inbox = boxes.open(&id("o"), None);
        let mgr = JobManager::default();
        mgr.start(inbox.notices(), job("kept"), "cmd".into())
            .finish(done(0, "ok"), Some("kept notice".into()));
        let stopped = mgr.start(inbox.notices(), job("stopped"), "cmd".into());
        mgr.cancel_owner(&id("o"));
        stopped.finish(done(0, "ok"), Some("stale notice".into()));
        mgr.start(boxes.notices_for(&id("gone")), job("orphan"), "cmd".into())
            .finish(done(0, "ok"), Some("nobody listens".into()));

        let notices = inbox.take();
        assert_eq!(notices.len(), 1);
        assert_eq!(
            (notices[0].body.as_str(), notices[0].kind),
            ("kept notice", Kind::Event)
        );
        assert_eq!(notices[0].from, Party::System);
        assert!(!mgr.has_running(&id("o")) && !mgr.has_running(&id("gone")));
    }

    #[test]
    fn an_earlier_run_finishing_late_leaves_the_rerun_under_its_id_running() {
        let mgr = JobManager::default();
        let boxes = Mailboxes::default();
        let first = mgr.start(boxes.notices_for(&id("o")), job("task-1"), "subagent".into());
        let _second = mgr.start(boxes.notices_for(&id("o")), job("task-1"), "subagent".into());

        first.finish(done(0, "stale"), None);

        assert_eq!(mgr.get(&job("task-1")).unwrap().status, JobStatus::Running);
        assert!(mgr.has_running(&id("o")));
    }

    #[test]
    fn running_jobs_only_count_for_their_own_session() {
        let mgr = JobManager::default();
        mgr.register(&id("session-a"), job("a1"), "sleep".into());
        mgr.register(&id("session-a"), job("a2"), "sleep".into());
        mgr.register(&id("session-b"), job("b1"), "sleep".into());

        assert_eq!(
            (mgr.running(&id("session-a")).len(), mgr.running(&id("session-b")).len()),
            (2, 1)
        );
        assert!(!mgr.has_running(&id("session-c")));

        mgr.update_status(&job("b1"), done(0, "ok"));
        assert!(!mgr.has_running(&id("session-b")));
        assert!(mgr.has_running(&id("session-a")));
    }

    #[test]
    fn cancelling_a_session_signals_only_its_running_jobs() {
        let mgr = JobManager::default();
        let a_running = mgr.register(&id("session-a"), job("a1"), "sleep".into());
        let a_finished = mgr.register(&id("session-a"), job("a2"), "true".into());
        let b_running = mgr.register(&id("session-b"), job("b1"), "sleep".into());
        mgr.update_status(&job("a2"), done(0, "ok"));

        mgr.cancel_owner(&id("session-a"));

        assert!(a_running.is_cancelled());
        assert!(!a_finished.is_cancelled(), "a finished job has nothing to stop");
        assert!(!b_running.is_cancelled());
    }

    #[test]
    fn a_reregistered_job_runs_again_and_is_not_evicted_as_finished() {
        let mgr = JobManager::default();
        mgr.register(&id("me"), job("task"), "subagent".into());
        mgr.update_status(&job("task"), done(0, "first round"));
        mgr.register(&id("me"), job("task"), "subagent".into());
        for i in 0..MAX_FINISHED_JOBS {
            let job_id = job(&format!("job-{i}"));
            mgr.register(&id("me"), job_id.clone(), "cmd".into());
            mgr.update_status(&job_id, done(0, "ok"));
        }
        assert_eq!(mgr.get(&job("task")).unwrap().status, JobStatus::Running);
        assert!(mgr.has_running(&id("me")));
    }

    #[test]
    fn finished_jobs_are_bounded_and_evict_their_logs() {
        let mgr = JobManager::default();
        let dir = tempfile::tempdir().unwrap();
        let first_log = dir.path().join("first.log");
        std::fs::write(&first_log, "output").unwrap();
        mgr.register(&id("me"), job("job-0"), "cmd".into());
        mgr.attach_log(&job("job-0"), first_log.clone());
        mgr.update_status(&job("job-0"), done(0, "ok"));
        for i in 1..=MAX_FINISHED_JOBS {
            let job_id = job(&format!("job-{i}"));
            mgr.register(&id("me"), job_id.clone(), "cmd".into());
            mgr.update_status(&job_id, done(0, "ok"));
        }
        assert!(mgr.get(&job("job-0")).is_none());
        assert!(!first_log.exists());
        assert_eq!(mgr.lock().jobs.len(), MAX_FINISHED_JOBS);
    }
}
