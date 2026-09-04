use std::collections::VecDeque;
use tokio::sync::mpsc;

use crate::engine::runner::{QUEUED_MESSAGE_BOUNDARY, QueuedMessageBoundary};
use crate::ui::interactive::{QueueKind, QueuedMessage};

mod runner;
mod types;

#[cfg(test)]
mod tests;

pub use runner::ReplAgentRunner;
pub use types::{ActivePromptRunner, ActiveQueueResult, CoordinatorInput, SharedSteeringQueue};

pub async fn run_active_queue<R>(
    initial: QueuedMessage,
    input: &mut mpsc::UnboundedReceiver<CoordinatorInput>,
    runner: &R,
) -> ActiveQueueResult<R::Error>
where
    R: ActivePromptRunner,
{
    debug_assert_eq!(QUEUED_MESSAGE_BOUNDARY, QueuedMessageBoundary::ActiveRunCompleted);
    let mut active = initial;
    let mut queued = VecDeque::new();
    let mut delivered = Vec::new();
    let mut deferred_commands = Vec::new();
    let mut accepting_input = true;

    loop {
        let run_result = {
            let run = runner.run_prompt(&active);
            tokio::pin!(run);
            loop {
                tokio::select! {
                    result = &mut run => break Some(result),
                    next = input.recv(), if accepting_input => {
                        match next {
                            Some(CoordinatorInput::Prompt(prompt)) => {
                                if prompt.text.starts_with('/') {
                                    deferred_commands.push(prompt.text);
                                } else if prompt.kind == QueueKind::Steering {
                                    let _ = runner.steer(&prompt).await;
                                    delivered.push(prompt);
                                } else {
                                    queued.push_back(prompt);
                                }
                            }
                            Some(CoordinatorInput::Command(command)) => deferred_commands.push(command),
                            Some(CoordinatorInput::Cancel) => break None,
                            None => accepting_input = false,
                        }
                    }
                }
            }
        };

        let Some(run_result) = run_result else {
            let cancellation_error = runner.cancel_active().await.err();
            drain_pending_input(input, &mut queued, &mut deferred_commands);
            return ActiveQueueResult::Cancelled {
                delivered,
                restored: queued.into(),
                deferred_commands,
                cancellation_error,
            };
        };

        if let Err(error) = run_result {
            drain_pending_input(input, &mut queued, &mut deferred_commands);
            return ActiveQueueResult::Failed {
                error,
                delivered,
                restored: queued.into(),
                deferred_commands,
            };
        }

        delivered.push(active);
        if drain_pending_input(input, &mut queued, &mut deferred_commands) {
            return ActiveQueueResult::Cancelled {
                delivered,
                restored: queued.into(),
                deferred_commands,
                cancellation_error: None,
            };
        }
        let Some(next) = queued.pop_front() else {
            return ActiveQueueResult::Completed {
                delivered,
                deferred_commands,
            };
        };
        active = next;
    }
}

fn drain_pending_input(
    input: &mut mpsc::UnboundedReceiver<CoordinatorInput>,
    queued: &mut VecDeque<QueuedMessage>,
    deferred_commands: &mut Vec<String>,
) -> bool {
    let mut cancelled = false;
    while let Ok(next) = input.try_recv() {
        match next {
            CoordinatorInput::Prompt(prompt) => {
                if prompt.text.starts_with('/') {
                    deferred_commands.push(prompt.text);
                } else {
                    queued.push_back(prompt);
                }
            }
            CoordinatorInput::Command(command) => deferred_commands.push(command),
            CoordinatorInput::Cancel => cancelled = true,
        }
    }
    cancelled
}
