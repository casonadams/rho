use super::conversation::ConversationSettings;
use crate::engine::runner::turn::types::CancellationSignal;
use crate::tools::bash::runner::neutralize_notice_tags;
use crate::tools::jobs::{JobManager, JobStatus, job_notice};
use crate::tools::types::ToolResult;
use rho_harness_core::presentation::{ToolStreamPort, ToolStreamSink};
use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;
use tokio::task::{AbortHandle, JoinHandle};

pub const DEFAULT_FOREGROUND_SECONDS: u64 = 60;
/// Subagent rounds that may run at once, in the foreground or the background.
const MAX_ACTIVE_ROUNDS: usize = 8;
/// Longest subagent result placed in a completion notice; the full record stays in the session.
const MAX_NOTICE_BYTES: usize = 20_000;

/// Subagent state that must survive rebuilding the engine's tools for the same session (a model or
/// login change): which conversations are busy, running rounds' inboxes, and the round count.
#[derive(Clone, Default)]
pub struct SubagentRuntime {
    busy: super::conversation::BusyContexts,
    inboxes: Inboxes,
    rounds: Arc<Rounds>,
}

impl SubagentRuntime {
    pub(crate) fn busy(&self) -> super::conversation::BusyContexts {
        self.busy.clone()
    }

    #[cfg(test)]
    pub(crate) fn shares_state_with(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.rounds, &other.rounds)
    }

    /// Queues a message the user typed for task `task_id`'s running round; `false` if none is running.
    pub fn message_from_user(&self, task_id: &str, text: &str) -> bool {
        self.inboxes.send(task_id, Sender::User, text)
    }

    /// Asks task `task_id`'s running round to stop and save what it has; `false` if none is running.
    pub fn stop(&self, task_id: &str) -> bool {
        self.inboxes.stop(task_id)
    }
}

/// Runs subagent rounds in the foreground up to a time budget, then moves them to the background.
#[derive(Clone)]
pub(crate) struct Backgrounding {
    jobs: JobManager,
    budget: Duration,
    inboxes: Inboxes,
    rounds: Arc<Rounds>,
}

impl Default for Backgrounding {
    fn default() -> Self {
        Self {
            jobs: JobManager::global().clone(),
            budget: Duration::from_secs(DEFAULT_FOREGROUND_SECONDS),
            inboxes: Inboxes::default(),
            rounds: Arc::default(),
        }
    }
}

/// Who sent a message to a running round.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Sender {
    DelegatingAgent,
    User,
}

impl Sender {
    pub(crate) fn heading(self) -> &'static str {
        match self {
            Self::DelegatingAgent => "Message from the delegating agent",
            Self::User => "Message from the user",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InboxMessage {
    pub from: Sender,
    pub text: String,
}

/// Messages sent to a running round; the subagent reads them before its next model call.
#[derive(Clone, Default)]
pub(crate) struct Inbox(Arc<Mutex<Vec<InboxMessage>>>);

impl Inbox {
    fn lock(&self) -> MutexGuard<'_, Vec<InboxMessage>> {
        self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    #[cfg(test)]
    pub(crate) fn push(&self, message: &str) {
        self.push_from(Sender::DelegatingAgent, message);
    }

    pub(crate) fn push_from(&self, from: Sender, text: &str) {
        self.lock().push(InboxMessage {
            from,
            text: text.to_string(),
        });
    }

    pub(crate) fn take_messages(&self) -> Vec<InboxMessage> {
        std::mem::take(&mut *self.lock())
    }

    pub(crate) fn take(&self) -> Vec<String> {
        self.take_messages().into_iter().map(|m| m.text).collect()
    }
}

/// A running round's inbox, the settings its task runs with, and the signal that stops it.
struct OpenRound {
    inbox: Inbox,
    settings: ConversationSettings,
    stop: CancellationSignal,
}

/// The running rounds by task id. A closed inbox accepts nothing more.
#[derive(Clone, Default)]
struct Inboxes(Arc<Mutex<HashMap<String, OpenRound>>>);

impl Inboxes {
    fn lock(&self) -> MutexGuard<'_, HashMap<String, OpenRound>> {
        self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn open(&self, task_id: &str, settings: ConversationSettings, stop: CancellationSignal) -> Inbox {
        let inbox = Inbox::default();
        let round = OpenRound {
            inbox: inbox.clone(),
            settings,
            stop,
        };
        self.lock().insert(task_id.to_string(), round);
        inbox
    }

    fn settings(&self, task_id: &str) -> Option<ConversationSettings> {
        self.lock().get(task_id).map(|round| round.settings.clone())
    }

    /// Asks task `task_id`'s running round to stop; `false` if none is running.
    fn stop(&self, task_id: &str) -> bool {
        let inboxes = self.lock();
        let Some(round) = inboxes.get(task_id) else {
            return false;
        };
        round.stop.cancel();
        true
    }

    fn send(&self, task_id: &str, from: Sender, message: &str) -> bool {
        let inboxes = self.lock();
        let Some(round) = inboxes.get(task_id) else {
            return false;
        };
        round.inbox.push_from(from, message);
        true
    }

    /// Closes `inbox` if it is still the task's open one, returning the messages the round never read.
    fn close(&self, task_id: &str, inbox: &Inbox) -> Vec<String> {
        let mut inboxes = self.lock();
        if inboxes
            .get(task_id)
            .is_some_and(|round| Arc::ptr_eq(&round.inbox.0, &inbox.0))
        {
            inboxes.remove(task_id);
        }
        inbox.take()
    }
}

/// A round's inbox, closed when the round ends or is aborted.
struct OpenInbox {
    inboxes: Inboxes,
    task_id: String,
    inbox: Option<Inbox>,
}

impl OpenInbox {
    fn close(&mut self) -> Vec<String> {
        self.inbox
            .take()
            .map(|inbox| self.inboxes.close(&self.task_id, &inbox))
            .unwrap_or_default()
    }
}

impl Drop for OpenInbox {
    fn drop(&mut self) {
        self.close();
    }
}

fn with_undelivered(mut result: ToolResult, undelivered: &[String]) -> ToolResult {
    if !undelivered.is_empty() {
        result
            .content
            .push_str("\n\nNot delivered (the subagent finished before reading them):");
        for message in undelivered {
            result.content.push_str(&format!("\n- {}", message.trim()));
        }
    }
    result
}

/// The subagent task a round belongs to.
pub(crate) struct BackgroundTask<'a> {
    pub id: &'a str,
    /// The session whose turns wait for the task and receive its result.
    pub owner: &'a str,
    /// Shown for the task in the job list.
    pub label: String,
    pub cancellation: Option<CancellationSignal>,
    /// What the task runs as; a message sent while it runs is checked against it.
    pub settings: ConversationSettings,
}

/// What a running round works with: where to stream progress, the inbox of messages sent to it,
/// and the signal asking it to stop (fired when its turn is interrupted or its session ends).
pub(crate) struct RoundIo {
    pub progress: ToolStreamPort,
    pub inbox: Inbox,
    pub stop: CancellationSignal,
}

/// How long a background round asked to stop may take to save its work before it is aborted.
const STOP_GRACE: Duration = Duration::from_secs(10);

/// Forwards progress to the parent's tool card only while the subagent runs in the foreground.
struct GatedSink {
    port: ToolStreamPort,
    open: Arc<AtomicBool>,
}

impl ToolStreamSink for GatedSink {
    fn tool_chunk(&self, chunk: String) {
        if self.open.load(Ordering::Relaxed) {
            self.port.stream_chunk(&chunk);
        }
    }
}

/// Stops the round if the parent drops the call while still waiting in the foreground.
struct AbortOnDrop(Option<AbortHandle>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        if let Some(handle) = self.0.take() {
            handle.abort();
        }
    }
}

fn joined(result: Result<ToolResult, tokio::task::JoinError>) -> ToolResult {
    result.unwrap_or_else(|e| ToolResult::error(format!("subagent stopped unexpectedly: {e}")))
}

fn moved_notice(task_id: &str, budget: Duration) -> String {
    format!(
        "Still running after {}s, so subagent task '{task_id}' moved to the background. Its result arrives as a follow-up message when it finishes, and your turn waits for it before ending. Do not poll or inspect it: continue other work now, or end your response to wait. To redirect it, call subagent with this task_id and a prompt; the message reaches it before its next model call.",
        budget.as_secs()
    )
}

fn clipped(text: &str) -> String {
    if text.len() <= MAX_NOTICE_BYTES {
        return text.to_string();
    }
    let mut end = MAX_NOTICE_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!(
        "{}\n[clipped {} more bytes; inspect the task for the full record]",
        &text[..end],
        text.len() - end
    )
}

fn completion_notice(task_id: &str, result: &ToolResult) -> String {
    let outcome = if result.is_error { "failed" } else { "finished" };
    job_notice(
        &format!("Background subagent task '{task_id}' {outcome}"),
        Some(&clipped(result.content.trim())),
    )
}

/// How many subagent rounds are running, and a wake-up for callers waiting for one to end.
#[derive(Default)]
struct Rounds {
    active: AtomicUsize,
    freed: tokio::sync::Notify,
}

/// Holds one of the limited slots for running rounds until dropped.
pub(crate) struct RoundSlot(Arc<Rounds>);

impl Rounds {
    fn try_take(self: &Arc<Self>) -> Option<RoundSlot> {
        self.active
            .try_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < MAX_ACTIVE_ROUNDS).then_some(n + 1)
            })
            .ok()
            .map(|_| RoundSlot(self.clone()))
    }

    async fn take(self: &Arc<Self>) -> RoundSlot {
        loop {
            let freed = self.freed.notified();
            tokio::pin!(freed);
            freed.as_mut().enable();
            if let Some(slot) = self.try_take() {
                return slot;
            }
            freed.await;
        }
    }

    #[cfg(test)]
    fn active(&self) -> usize {
        self.active.load(Ordering::Acquire)
    }
}

impl Drop for RoundSlot {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::AcqRel);
        self.0.freed.notify_waiters();
    }
}

impl Backgrounding {
    #[must_use]
    pub(crate) fn with_budget(mut self, budget: Duration) -> Self {
        self.budget = budget;
        self
    }

    /// A slot for a new round, or the refusal to give the caller when all are taken.
    pub(crate) fn reserve(&self) -> Result<RoundSlot, String> {
        self.rounds.try_take().ok_or_else(|| {
            format!("{MAX_ACTIVE_ROUNDS} subagents are already running; wait for their results before starting another")
        })
    }

    /// Waits until a round slot is free; workflow steps queue here instead of failing.
    pub(crate) async fn wait_for_slot(&self) -> RoundSlot {
        self.rounds.take().await
    }

    /// Counts rounds and routes messages through `runtime`, shared with other tools of the session.
    #[must_use]
    pub(crate) fn sharing(mut self, runtime: &SubagentRuntime) -> Self {
        self.inboxes = runtime.inboxes.clone();
        self.rounds = runtime.rounds.clone();
        self
    }

    #[cfg(test)]
    pub(crate) fn new(jobs: JobManager, notify: crate::tools::jobs::Notifier, budget: Duration) -> Self {
        Self {
            jobs: jobs.delivering_to(notify),
            budget,
            inboxes: Inboxes::default(),
            rounds: Arc::default(),
        }
    }

    /// What task `task_id` runs as while one of its rounds is running.
    pub(crate) fn running_settings(&self, task_id: &str) -> Option<ConversationSettings> {
        self.inboxes.settings(task_id)
    }

    /// Queues `message` for task `task_id` if one of its rounds is running, returning the confirmation.
    pub(crate) fn steer(&self, task_id: &str, message: &str) -> Option<String> {
        self.inboxes.send(task_id, Sender::DelegatingAgent, message).then(|| {
            format!("Message queued for running subagent task '{task_id}'; it reads it before its next model call.")
        })
    }

    /// A message for task `task_id` while its round still runs in the background, otherwise `None`.
    pub(crate) fn still_running(&self, task_id: &str) -> Option<String> {
        self.jobs
            .get(task_id)
            .filter(|job| job.status == JobStatus::Running)
            .map(|_| format!("subagent task '{task_id}' is still running in the background; its result arrives as a follow-up message when it finishes"))
    }

    /// Runs `work` for the task in the reserved `slot`, returning its result if it finishes within the
    /// budget, otherwise a notice that it continues in the background. A round that ends after
    /// `cancellation` fires reports nothing.
    pub(crate) async fn run<F>(
        &self,
        slot: RoundSlot,
        task: BackgroundTask<'_>,
        progress: &ToolStreamPort,
        work: impl FnOnce(RoundIo) -> F,
    ) -> ToolResult
    where
        F: Future<Output = ToolResult> + Send + 'static,
    {
        let open = Arc::new(AtomicBool::new(true));
        let gated = ToolStreamPort::new(Some(Arc::new(GatedSink {
            port: progress.clone(),
            open: open.clone(),
        })));
        let stop = CancellationSignal::default();
        let inbox = self.inboxes.open(task.id, task.settings.clone(), stop.clone());
        let mut open_inbox = OpenInbox {
            inboxes: self.inboxes.clone(),
            task_id: task.id.to_string(),
            inbox: Some(inbox.clone()),
        };
        let forward_turn_stop = AbortOnDrop(task.cancellation.clone().map(|turn| {
            let stop = stop.clone();
            tokio::spawn(async move {
                turn.wait_cancelled().await;
                stop.cancel();
            })
            .abort_handle()
        }));
        let work = work(RoundIo {
            progress: gated,
            inbox,
            stop: stop.clone(),
        });
        let mut handle = tokio::spawn(async move {
            let _slot = slot;
            let _forward = forward_turn_stop;
            let result = work.await;
            with_undelivered(result, &open_inbox.close())
        });
        let mut guard = AbortOnDrop(Some(handle.abort_handle()));
        match tokio::time::timeout(self.budget, &mut handle).await {
            Ok(result) => joined(result),
            Err(_) => {
                guard.0 = None;
                open.store(false, Ordering::Relaxed);
                self.detach(handle, &task, stop);
                ToolResult::success(moved_notice(task.id, self.budget))
            }
        }
    }

    /// Tracks a round that moved to the background. Stopping its job asks the round to stop so it can
    /// save its transcript, and aborts it only if it has not finished after `STOP_GRACE`.
    fn detach(&self, mut handle: JoinHandle<ToolResult>, task: &BackgroundTask<'_>, stop: CancellationSignal) {
        let job = self.jobs.start(task.owner, task.id.to_string(), task.label.clone());
        let cancellation = task.cancellation.clone();
        tokio::spawn(async move {
            let result = tokio::select! {
                result = &mut handle => joined(result),
                () = job.cancel().wait_cancelled() => {
                    stop.cancel();
                    match tokio::time::timeout(STOP_GRACE, &mut handle).await {
                        Ok(result) => joined(result),
                        Err(_) => {
                            handle.abort();
                            ToolResult::error("subagent stopped: the turn was interrupted or its session ended")
                        }
                    }
                }
            };
            let turn_stopped = cancellation.as_ref().is_some_and(CancellationSignal::is_cancelled);
            let notice = (!turn_stopped).then(|| completion_notice(job.id(), &result));
            let content = neutralize_notice_tags(&result.content);
            let status = if result.is_error {
                JobStatus::Failed { error: content }
            } else {
                JobStatus::Completed {
                    exit_code: 0,
                    output: content,
                }
            };
            job.finish(status, notice);
        });
    }
}

#[cfg(test)]
#[path = "background_tests.rs"]
mod tests;
