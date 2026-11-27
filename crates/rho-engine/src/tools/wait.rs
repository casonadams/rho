use crate::engine::runner::SteeringQueueProvider;
use crate::engine::runner::turn::types::{CancellationSignal, message_waiting, unless_cancelled};
use crate::mailbox::Mailbox;
use crate::tools::engine_tool::{EngineTool, TurnToolContext, TypedTool};
use crate::tools::jobs::{BackgroundJobInfo, Wakeups};
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
        message_waiting(&self.inbox, self.steering.as_deref())
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
        let running = turn.inbox.has_running_jobs();
        let notices = turn.inbox.read_notices();
        if !notices.is_empty() {
            return Some(ToolResult::success(notices.join("\n\n")));
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

    fn still_running(inbox: &Mailbox) -> ToolResult {
        let entries: Vec<String> = inbox.jobs().running(inbox.id()).iter().map(describe).collect();
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
        let mut wakeups = Wakeups::new(&turn.inbox, turn.steering.as_deref());
        let mut waited = false;
        loop {
            if let Some(result) = self.ready(turn, waited) {
                return Ok(result);
            }
            waited = true;
            let woken = tokio::time::timeout_at(deadline, wakeups.next());
            if let Some(Err(_)) = unless_cancelled(turn.cancellation.as_ref(), woken).await {
                return Ok(self
                    .ready(turn, waited)
                    .unwrap_or_else(|| Self::still_running(&turn.inbox)));
            }
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
