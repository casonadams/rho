use crate::engine::runner::SteeringQueueProvider;
use crate::engine::runner::turn::types::CancellationSignal;
use crate::mailbox::{Mailbox, NoticeTarget};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;
use tokio::sync::watch;

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
    /// The session whose turns wait for this job and receive its notice.
    pub owner: String,
    pub command: String,
    pub started_at: Instant,
    pub status: JobStatus,
    pub log_path: Option<PathBuf>,
    pub cancel: CancellationSignal,
}

#[derive(Default)]
struct Registry {
    jobs: HashMap<String, BackgroundJobInfo>,
    finished: VecDeque<String>,
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

/// What wakes an agent waiting on its background work: a message in its mailbox, steering from the
/// user, or any job ending. Made before the waiter first checks what it waits for, so it never
/// misses a change that happens between that check and the wait.
pub(crate) struct Wakeups<'a> {
    inbox: Option<&'a Mailbox>,
    steering: Option<&'a dyn SteeringQueueProvider>,
    ended: watch::Receiver<u64>,
}

impl Wakeups<'_> {
    /// Returns once a message or steering is waiting, or after a job ended since the last call. An
    /// agent that blocks here is not answering, so it first declines the questions it has read.
    pub(crate) async fn next(&mut self) {
        let Self { inbox, steering, ended } = self;
        if let Some(inbox) = inbox {
            inbox.decline_unanswered();
        }
        let job_ended = async {
            if ended.changed().await.is_err() {
                std::future::pending::<()>().await;
            }
        };
        let message = async {
            match inbox {
                Some(inbox) => inbox.wait().await,
                None => std::future::pending().await,
            }
        };
        let steering = async {
            match steering {
                Some(steering) => steering.arrived().await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            () = job_ended => {}
            () = message => {}
            () = steering => {}
        }
    }
}

/// A running job's handle. Finishing it records the outcome; dropping it unfinished (a panic or an
/// aborted task) marks the job failed so no turn waits on it forever.
pub(crate) struct JobCompletion {
    jobs: JobManager,
    id: String,
    /// Where the owner's notice goes; `None` for a job nobody is told about.
    owner: Option<NoticeTarget>,
    cancel: CancellationSignal,
    finished: bool,
}

impl JobCompletion {
    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    /// Fires when the job must stop: its turn was interrupted or its session ended.
    pub(crate) fn cancel(&self) -> &CancellationSignal {
        &self.cancel
    }

    /// Records `status`; `notice` reaches the owner's mailbox unless the job was stopped. The notice
    /// is sent before the job stops counting as running, so a waiting turn cannot miss it.
    pub(crate) fn finish(mut self, status: JobStatus, notice: Option<String>) {
        self.finished = true;
        if let (Some(owner), Some(notice)) = (&self.owner, notice.filter(|_| !self.cancel.is_cancelled()))
            && let Err(error) = owner.send(notice)
        {
            tracing::warn!("delivering the notice for job {} failed: {error}", self.id);
        }
        self.jobs.update_status(&self.id, status);
    }
}

impl Drop for JobCompletion {
    fn drop(&mut self) {
        if !self.finished {
            let error = "stopped unexpectedly before reporting a result".to_string();
            self.jobs.update_status(&self.id, JobStatus::Failed { error });
        }
    }
}

static GLOBAL_JOB_MANAGER: std::sync::LazyLock<JobManager> = std::sync::LazyLock::new(JobManager::default);

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
    pub fn global() -> &'static Self {
        &GLOBAL_JOB_MANAGER
    }

    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> MutexGuard<'_, Registry> {
        self.registry.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Registers a running job for the session `owner`. The returned signal fires when the job must stop.
    pub fn register(&self, owner: &str, id: String, command: String) -> CancellationSignal {
        let cancel = CancellationSignal::default();
        let mut registry = self.lock();
        registry.finished.retain(|finished| *finished != id);
        registry.jobs.insert(
            id.clone(),
            BackgroundJobInfo {
                id,
                owner: owner.to_string(),
                command,
                started_at: Instant::now(),
                status: JobStatus::Running,
                log_path: None,
                cancel: cancel.clone(),
            },
        );
        cancel
    }

    /// Registers a running job for the agent `owner` (none for a job nobody waits on) and returns the
    /// handle that must finish it; its notice goes to `owner`.
    pub(crate) fn start(&self, owner: Option<NoticeTarget>, id: String, command: String) -> JobCompletion {
        let cancel = self.register(owner.as_ref().map_or("", NoticeTarget::agent), id.clone(), command);
        JobCompletion {
            jobs: self.clone(),
            id,
            owner,
            cancel,
            finished: false,
        }
    }

    pub fn attach_log(&self, id: &str, path: PathBuf) {
        if let Some(job) = self.lock().jobs.get_mut(id) {
            job.log_path = Some(path);
        }
    }

    pub fn update_status(&self, id: &str, status: JobStatus) {
        let mut registry = self.lock();
        let Registry { jobs, finished } = &mut *registry;
        let mut ended = false;
        if let Some(job) = jobs.get_mut(id) {
            ended = job.status == JobStatus::Running && status != JobStatus::Running;
            job.status = status;
            if ended {
                finished.push_back(id.to_string());
            }
        }
        evict_finished(&mut registry);
        drop(registry);
        if ended {
            self.ended.send_modify(|count| *count += 1);
        }
    }

    /// Wakes a waiter on `inbox`, `steering`, or any job of this manager ending.
    pub(crate) fn wakeups<'a>(
        &self,
        inbox: Option<&'a Mailbox>,
        steering: Option<&'a dyn SteeringQueueProvider>,
    ) -> Wakeups<'a> {
        Wakeups {
            inbox,
            steering,
            ended: self.ended.subscribe(),
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

    pub fn running_count(&self, owner: &str) -> usize {
        self.lock()
            .jobs
            .values()
            .filter(|j| j.owner == owner && matches!(j.status, JobStatus::Running))
            .count()
    }

    pub fn has_running(&self, owner: &str) -> bool {
        self.running_count(owner) > 0
    }

    /// `owner`'s running jobs, oldest first.
    pub fn running(&self, owner: &str) -> Vec<BackgroundJobInfo> {
        let mut running = self.list();
        running.retain(|j| j.owner == owner && j.status == JobStatus::Running);
        running
    }

    /// Signals every running job of `owner` to stop; each job records its own failure when it does.
    pub fn cancel_owner(&self, owner: &str) {
        let running: Vec<CancellationSignal> = self
            .lock()
            .jobs
            .values()
            .filter(|j| j.owner == owner && matches!(j.status, JobStatus::Running))
            .map(|j| j.cancel.clone())
            .collect();
        for cancel in running {
            cancel.cancel();
        }
    }
}

/// Stops a no-longer-active session's background work. Its undelivered notices go with its mailbox.
pub fn abandon_background_work(owner: &str) {
    JobManager::global().cancel_owner(owner);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mailbox::{Kind, Mailboxes, Party};

    fn done(exit_code: i32, output: &str) -> JobStatus {
        JobStatus::Completed {
            exit_code,
            output: output.into(),
        }
    }

    #[test]
    fn test_job_manager_lifecycle() {
        let mgr = JobManager::new();
        mgr.register("me", "j1".into(), "echo 1".into());
        assert_eq!(mgr.get("j1").unwrap().status, JobStatus::Running);
        assert!(mgr.has_running("me"));
        assert_eq!(mgr.running_count("me"), 1);

        mgr.update_status("j1", done(0, "ok"));
        assert_eq!(mgr.get("j1").unwrap().status, done(0, "ok"));
        assert!(!mgr.has_running("me"));
        assert_eq!(mgr.running_count("me"), 0);

        assert!(!mgr.list().is_empty());
    }

    #[tokio::test]
    async fn a_job_whose_task_dies_before_reporting_stops_blocking_its_session() {
        let mgr = JobManager::new();
        let job = mgr.start(
            Some(crate::mailbox::Mailboxes::default().notices_for("panicking-owner")),
            "p1".into(),
            "cmd".into(),
        );
        let task = tokio::spawn(async move {
            let _job = job;
            panic!("task died");
        });
        assert!(task.await.unwrap_err().is_panic());

        assert!(!mgr.has_running("panicking-owner"));
        assert!(matches!(
            mgr.get("p1").unwrap().status,
            JobStatus::Failed { error } if error.contains("stopped unexpectedly")
        ));
    }

    #[test]
    fn a_finished_jobs_notice_reaches_its_owners_mailbox_unless_the_job_was_told_to_stop() {
        let boxes = Mailboxes::default();
        let inbox = boxes.open("o", None);
        let mgr = JobManager::new();
        mgr.start(Some(inbox.notices()), "kept".into(), "cmd".into())
            .finish(done(0, "ok"), Some("kept notice".into()));
        let stopped = mgr.start(Some(inbox.notices()), "stopped".into(), "cmd".into());
        mgr.cancel_owner("o");
        stopped.finish(done(0, "ok"), Some("stale notice".into()));
        mgr.start(Some(boxes.notices_for("gone")), "orphan".into(), "cmd".into())
            .finish(done(0, "ok"), Some("nobody listens".into()));

        let notices = inbox.take();
        assert_eq!(notices.len(), 1);
        assert_eq!(
            (notices[0].body.as_str(), notices[0].kind),
            ("kept notice", Kind::Event)
        );
        assert_eq!(notices[0].from, Party::System);
        assert!(!mgr.has_running("o") && !mgr.has_running("gone"));
    }

    #[test]
    fn running_jobs_only_count_for_their_own_session() {
        let mgr = JobManager::new();
        mgr.register("session-a", "a1".into(), "sleep".into());
        mgr.register("session-a", "a2".into(), "sleep".into());
        mgr.register("session-b", "b1".into(), "sleep".into());

        assert_eq!((mgr.running_count("session-a"), mgr.running_count("session-b")), (2, 1));
        assert!(!mgr.has_running("session-c"));

        mgr.update_status("b1", done(0, "ok"));
        assert!(!mgr.has_running("session-b"));
        assert!(mgr.has_running("session-a"));
    }

    #[test]
    fn cancelling_a_session_signals_only_its_running_jobs() {
        let mgr = JobManager::new();
        let a_running = mgr.register("session-a", "a1".into(), "sleep".into());
        let a_finished = mgr.register("session-a", "a2".into(), "true".into());
        let b_running = mgr.register("session-b", "b1".into(), "sleep".into());
        mgr.update_status("a2", done(0, "ok"));

        mgr.cancel_owner("session-a");

        assert!(a_running.is_cancelled());
        assert!(!a_finished.is_cancelled(), "a finished job has nothing to stop");
        assert!(!b_running.is_cancelled());
    }

    #[test]
    fn abandoning_a_session_cancels_only_its_jobs() {
        let global = JobManager::global();
        let gone = global.register("abandon-gone", "abandon-job-gone".into(), "sleep".into());
        let kept = global.register("abandon-kept", "abandon-job-kept".into(), "sleep".into());

        abandon_background_work("abandon-gone");

        assert!(gone.is_cancelled());
        assert!(!kept.is_cancelled());
        global.update_status("abandon-job-gone", done(0, ""));
        global.update_status("abandon-job-kept", done(0, ""));
    }

    #[test]
    fn a_reregistered_job_runs_again_and_is_not_evicted_as_finished() {
        let mgr = JobManager::new();
        mgr.register("me", "task".into(), "subagent".into());
        mgr.update_status("task", done(0, "first round"));
        mgr.register("me", "task".into(), "subagent".into());
        for i in 0..MAX_FINISHED_JOBS {
            let id = format!("job-{i}");
            mgr.register("me", id.clone(), "cmd".into());
            mgr.update_status(&id, done(0, "ok"));
        }
        assert_eq!(mgr.get("task").unwrap().status, JobStatus::Running);
        assert!(mgr.has_running("me"));
    }

    #[test]
    fn finished_jobs_are_bounded_and_evict_their_logs() {
        let mgr = JobManager::new();
        let dir = tempfile::tempdir().unwrap();
        let first_log = dir.path().join("first.log");
        std::fs::write(&first_log, "output").unwrap();
        mgr.register("me", "job-0".into(), "cmd".into());
        mgr.attach_log("job-0", first_log.clone());
        mgr.update_status("job-0", done(0, "ok"));
        for i in 1..=MAX_FINISHED_JOBS {
            let id = format!("job-{i}");
            mgr.register("me", id.clone(), "cmd".into());
            mgr.update_status(&id, done(0, "ok"));
        }
        assert!(mgr.get("job-0").is_none());
        assert!(!first_log.exists());
        assert_eq!(mgr.list().len(), MAX_FINISHED_JOBS);
    }
}
