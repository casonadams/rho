//! Runs a subagent round in the foreground up to a time budget, then as a background job whose
//! result reaches the delegating agent's mailbox as a notice.

use super::conversation::ConversationSettings;
use super::runtime::{RoundSlot, SubagentRuntime};
use crate::engine::runner::turn::types::CancellationSignal;
use crate::mailbox::{AgentId, Mailbox, framed_notice, neutralize_notice_tags};
use crate::tools::jobs::{JobId, JobStatus};
use crate::tools::types::ToolResult;
use rho_harness_core::presentation::{Presenter, SUBAGENT_STARTED_MARKER, ToolStreamPort, ToolStreamSink};
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::task::{AbortHandle, JoinHandle};

pub const DEFAULT_FOREGROUND_SECONDS: u64 = 0;
/// Longest subagent result placed in a completion notice; the full record stays in the session.
const MAX_NOTICE_BYTES: usize = 20_000;
/// How long a background round asked to stop may take to save its work before it is aborted.
const STOP_GRACE: Duration = Duration::from_secs(10);

/// Runs subagent rounds in the foreground up to a time budget, then moves them to the background.
#[derive(Clone)]
pub(crate) struct Backgrounding {
    budget: Duration,
    runtime: SubagentRuntime,
}

impl Default for Backgrounding {
    fn default() -> Self {
        Self {
            budget: Duration::from_secs(DEFAULT_FOREGROUND_SECONDS),
            runtime: SubagentRuntime::default(),
        }
    }
}

/// The subagent task a round belongs to.
pub(crate) struct BackgroundTask<'a> {
    pub id: &'a AgentId,
    /// The session whose turns wait for the task and receive its result.
    pub owner: &'a AgentId,
    /// Shown for the task in the job list.
    pub label: String,
    pub cancellation: Option<CancellationSignal>,
    /// What the task runs as; a message sent while it runs is checked against it.
    pub settings: ConversationSettings,
    /// Optional presenter for emitting live `rho:subagents` A2UI surface updates.
    pub presenter: Option<Arc<dyn Presenter>>,
}

/// What a running round works with: where to stream progress, its mailbox, and the signal asking it
/// to stop (fired when its turn is interrupted or its session ends).
pub(crate) struct RoundIo {
    pub progress: ToolStreamPort,
    pub inbox: Mailbox,
    pub stop: CancellationSignal,
}

/// Forwards progress to the parent's tool card only while the subagent runs in the foreground.
struct GatedSink {
    port: ToolStreamPort,
    open: Arc<AtomicBool>,
}

impl GatedSink {
    /// A port forwarding to `port` until the returned gate is closed.
    fn over(port: &ToolStreamPort) -> (ToolStreamPort, Arc<AtomicBool>) {
        let open = Arc::new(AtomicBool::new(true));
        let sink = Self {
            port: port.clone(),
            open: open.clone(),
        };
        (ToolStreamPort::new(Some(Arc::new(sink))), open)
    }
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

/// Runs `work` to its end, firing `stop` when the delegating agent's `turn` is interrupted so the
/// round can wrap up instead of being cut off.
async fn stopped_with_turn<T>(
    turn: Option<CancellationSignal>,
    stop: &CancellationSignal,
    work: impl Future<Output = T>,
) -> T {
    tokio::pin!(work);
    if let Some(turn) = turn {
        tokio::select! {
            result = &mut work => return result,
            () = turn.wait_cancelled() => stop.cancel(),
        }
    }
    work.await
}

fn started_notice(task_id: &str, settings: &ConversationSettings) -> String {
    let agent = settings.agent_or_role();
    format!(
        "{SUBAGENT_STARTED_MARKER}{task_id}' ({agent}). Its result arrives as a follow-up message when it finishes; call wait if you need it now, or continue other work or end your turn. To redirect it, call subagent with this task_id and a prompt."
    )
}

fn clipped(text: &str) -> String {
    if text.len() <= MAX_NOTICE_BYTES {
        return text.to_string();
    }
    let end = text.floor_char_boundary(MAX_NOTICE_BYTES);
    format!(
        "{}\n[clipped {} more bytes; inspect the task for the full record]",
        &text[..end],
        text.len() - end
    )
}

fn completion_notice(task_id: &str, result: &ToolResult) -> String {
    let outcome = if result.is_error { "failed" } else { "finished" };
    framed_notice(
        &format!("Background subagent task '{task_id}' {outcome}"),
        Some(&clipped(result.content.trim())),
    )
}

impl Backgrounding {
    #[must_use]
    pub(crate) fn with_budget(mut self, budget: Duration) -> Self {
        self.budget = budget;
        self
    }

    /// Runs rounds under `runtime`, shared with the other tools of the session.
    #[must_use]
    pub(crate) fn sharing(mut self, runtime: &SubagentRuntime) -> Self {
        self.runtime = runtime.clone();
        self
    }

    pub(crate) fn runtime(&self) -> &SubagentRuntime {
        &self.runtime
    }

    #[cfg(test)]
    pub(crate) fn new(budget: Duration) -> Self {
        Self {
            budget,
            ..Self::default()
        }
    }

    /// A message for task `task_id` while its round still runs in the background, otherwise `None`.
    pub(crate) fn still_running(&self, task_id: &str) -> Option<String> {
        self.runtime
            .jobs()
            .get(&JobId::from(task_id))
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
        let (gated, foreground) = GatedSink::over(progress);
        let stop = CancellationSignal::default();
        let mut round = self.runtime.open_round(
            task.id,
            task.owner,
            task.settings.clone(),
            task.presenter.clone(),
            stop.clone(),
        );
        let inbox = round.mailbox();
        let work = work(RoundIo {
            progress: gated,
            inbox: inbox.clone(),
            stop: stop.clone(),
        });
        let turn = task.cancellation.clone();
        let round_stop = stop.clone();
        let mut handle = tokio::spawn(async move {
            let _slot = slot;
            let result = stopped_with_turn(turn, &round_stop, work).await;
            with_undelivered(result, &round.close())
        });
        let mut guard = AbortOnDrop(Some(handle.abort_handle()));
        match self.wait_inline(&mut handle, &inbox).await {
            Some(result) => joined(result),
            None => {
                guard.0 = None;
                foreground.store(false, Ordering::Relaxed);
                self.detach(handle, &task, stop);
                ToolResult::success(started_notice(task.id.as_str(), &task.settings))
            }
        }
    }

    /// The round's outcome if it ends within the foreground budget, before it asks the delegating
    /// agent a question (which that agent can only answer once this call returns); a zero budget
    /// never waits.
    async fn wait_inline(
        &self,
        handle: &mut JoinHandle<ToolResult>,
        inbox: &Mailbox,
    ) -> Option<Result<ToolResult, tokio::task::JoinError>> {
        if self.budget.is_zero() {
            return None;
        }
        tokio::select! {
            result = tokio::time::timeout(self.budget, handle) => result.ok(),
            () = inbox.asked() => None,
        }
    }

    /// Tracks a round that moved to the background. Stopping its job asks the round to stop so it can
    /// save its transcript, and aborts it only if it has not finished after `STOP_GRACE`.
    fn detach(&self, mut handle: JoinHandle<ToolResult>, task: &BackgroundTask<'_>, stop: CancellationSignal) {
        let job = self
            .runtime
            .start_job(task.owner, JobId::from(task.id.as_str()), task.label.clone());
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
            let notice = (!turn_stopped).then(|| completion_notice(job.id().as_str(), &result));
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
