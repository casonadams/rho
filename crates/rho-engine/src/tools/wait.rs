use crate::engine::runner::SteeringQueueProvider;
use crate::engine::runner::turn::types::{CancellationSignal, unless_cancelled};
use crate::mailbox::{Mailbox, wait_for_message_or_tick};
use crate::tools::engine_tool::{EngineTool, TurnToolContext, TypedTool};
use crate::tools::jobs::{BackgroundJobInfo, JobManager};
use crate::tools::types::ToolResult;
use rho_harness_core::args::WaitArgs;
use rho_harness_core::error::AppError;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::Instant;

/// Longest a single call blocks before reporting what still runs.
const WAIT_CAP: Duration = Duration::from_secs(600);
const NOTHING_RUNNING: &str = "Nothing is running; there is nothing to wait for.";
const COMMAND_PREVIEW_CHARS: usize = 60;

/// Blocks until the agent's background work reports, a message arrives, or the turn is interrupted.
/// Each turn binds a copy to the agent's mailbox; an unbound tool has nothing to wait for.
#[derive(Clone)]
pub struct WaitTool {
    jobs: JobManager,
    cap: Duration,
    turn: Option<Turn>,
}

#[derive(Clone)]
struct Turn {
    inbox: Mailbox,
    cancellation: Option<CancellationSignal>,
    steering: Option<Arc<dyn SteeringQueueProvider>>,
}

impl Default for WaitTool {
    fn default() -> Self {
        Self {
            jobs: JobManager::global().clone(),
            cap: WAIT_CAP,
            turn: None,
        }
    }
}

impl Turn {
    fn interrupted(&self) -> bool {
        self.cancellation.as_ref().is_some_and(CancellationSignal::is_cancelled)
    }

    fn message_waiting(&self) -> bool {
        self.inbox.pending() > 0 || self.steering.as_ref().is_some_and(|s| s.has_steering())
    }
}

impl WaitTool {
    /// What ends the wait now, if anything. Running work is read before the notices so a job that
    /// finished (its notice is sent before it stops running) cannot be missed.
    fn ready(&self, turn: &Turn, waited: bool) -> Option<ToolResult> {
        if turn.interrupted() {
            return Some(ToolResult::error(
                "Wait interrupted; background work keeps running and its result still arrives as a follow-up message.",
            ));
        }
        let running = self.jobs.has_running(turn.inbox.id());
        let notices = turn.inbox.take_notices();
        if !notices.is_empty() {
            return Some(ToolResult::success(turn.inbox.for_model(&notices).join("\n\n")));
        }
        if turn.message_waiting() {
            return Some(ToolResult::success(
                "Wait ended: a new message arrived and follows this result.",
            ));
        }
        (!running).then(|| {
            ToolResult::success(if waited {
                "The background work stopped without a result to report."
            } else {
                NOTHING_RUNNING
            })
        })
    }

    fn still_running(&self, owner: &str) -> ToolResult {
        let entries: Vec<String> = self.jobs.running(owner).iter().map(describe).collect();
        ToolResult::success(format!(
            "Still running: {}. Call wait again to keep waiting, or end your turn.",
            entries.join("; ")
        ))
    }
}

fn describe(job: &BackgroundJobInfo) -> String {
    let command: String = job.command.chars().take(COMMAND_PREVIEW_CHARS).collect();
    format!(
        "{} ({}, {}s)",
        job.id,
        command.trim(),
        job.started_at.elapsed().as_secs()
    )
}

#[async_trait::async_trait]
impl TypedTool for WaitTool {
    type Args = WaitArgs;
    const NAME: &'static str = "wait";

    async fn run(&self, _args: WaitArgs) -> Result<ToolResult, AppError> {
        let Some(turn) = &self.turn else {
            return Ok(ToolResult::success(NOTHING_RUNNING));
        };
        let deadline = Instant::now() + self.cap;
        let mut waited = false;
        loop {
            if let Some(result) = self.ready(turn, waited) {
                return Ok(result);
            }
            if Instant::now() >= deadline {
                return Ok(self.still_running(turn.inbox.id()));
            }
            waited = true;
            unless_cancelled(turn.cancellation.as_ref(), wait_for_message_or_tick(Some(&turn.inbox))).await;
        }
    }

    fn bind_to_turn(&self, ctx: &TurnToolContext) -> Option<Arc<dyn EngineTool>> {
        let inbox = ctx.inbox.clone()?;
        Some(Arc::new(Self {
            turn: Some(Turn {
                inbox,
                cancellation: ctx.cancellation.clone(),
                steering: ctx.steering.clone(),
            }),
            ..self.clone()
        }))
    }
}

#[cfg(test)]
#[path = "wait_tests.rs"]
mod tests;
