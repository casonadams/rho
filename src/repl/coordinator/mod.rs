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

struct QueueCoordinatorState {
    queued: VecDeque<QueuedMessage>,
    delivered: Vec<QueuedMessage>,
    deferred_commands: Vec<String>,
}

impl QueueCoordinatorState {
    fn new() -> Self {
        Self {
            queued: VecDeque::new(),
            delivered: Vec::new(),
            deferred_commands: Vec::new(),
        }
    }

    fn into_cancelled<E>(self, cancellation_error: Option<E>) -> ActiveQueueResult<E> {
        ActiveQueueResult::Cancelled {
            delivered: self.delivered,
            restored: self.queued.into(),
            deferred_commands: self.deferred_commands,
            cancellation_error,
        }
    }

    fn handle_cancelled<E>(
        mut self,
        cancellation_error: Option<E>,
        input: &mut mpsc::UnboundedReceiver<CoordinatorInput>,
    ) -> ActiveQueueResult<E> {
        drain_pending_input(input, &mut self.queued, &mut self.deferred_commands);
        self.into_cancelled(cancellation_error)
    }

    fn handle_failed<E>(
        mut self,
        error: E,
        input: &mut mpsc::UnboundedReceiver<CoordinatorInput>,
    ) -> ActiveQueueResult<E> {
        drain_pending_input(input, &mut self.queued, &mut self.deferred_commands);
        ActiveQueueResult::Failed {
            error,
            delivered: self.delivered,
            restored: self.queued.into(),
            deferred_commands: self.deferred_commands,
        }
    }
}

async fn handle_coordinator_input<R: ActivePromptRunner>(
    msg: CoordinatorInput,
    runner: &R,
    state: &mut QueueCoordinatorState,
) -> bool {
    match msg {
        CoordinatorInput::Prompt(prompt) => {
            if crate::repl::commands::is_slash_command(&prompt.text) {
                state.deferred_commands.push(prompt.text);
            } else if prompt.kind == QueueKind::Steering {
                let _ = runner.steer(&prompt).await;
                state.delivered.push(prompt);
            } else {
                state.queued.push_back(prompt);
            }
            false
        }
        CoordinatorInput::Command(cmd) => {
            state.deferred_commands.push(cmd);
            false
        }
        CoordinatorInput::Cancel => true,
    }
}

enum RunPromptEvent<E> {
    Finished(Result<(), E>),
    Input(Option<CoordinatorInput>),
}

async fn poll_run_prompt<R: ActivePromptRunner, F: std::future::Future<Output = Result<(), R::Error>> + Unpin>(
    run: &mut F,
    input: &mut mpsc::UnboundedReceiver<CoordinatorInput>,
    accepting: bool,
) -> RunPromptEvent<R::Error> {
    tokio::select! {
        res = run => RunPromptEvent::Finished(res),
        next = input.recv(), if accepting => RunPromptEvent::Input(next),
    }
}

async fn wait_for_prompt_run<R: ActivePromptRunner>(
    (runner, active): (&R, &QueuedMessage),
    input: &mut mpsc::UnboundedReceiver<CoordinatorInput>,
    state: &mut QueueCoordinatorState,
) -> Option<Result<(), R::Error>> {
    let run = runner.run_prompt(active);
    tokio::pin!(run);
    let mut accepting = true;
    loop {
        match poll_run_prompt::<R, _>(&mut run, input, accepting).await {
            RunPromptEvent::Finished(res) => return Some(res),
            RunPromptEvent::Input(Some(msg)) => {
                if handle_coordinator_input(msg, runner, state).await {
                    return None;
                }
            }
            RunPromptEvent::Input(None) => accepting = false,
        }
    }
}

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
    let mut state = QueueCoordinatorState::new();

    loop {
        let Some(run_result) = wait_for_prompt_run((runner, &active), input, &mut state).await else {
            return state.handle_cancelled(runner.cancel_active().await.err(), input);
        };

        if let Err(error) = run_result {
            return state.handle_failed(error, input);
        }

        state.delivered.push(active);
        if drain_pending_input(input, &mut state.queued, &mut state.deferred_commands) {
            return state.into_cancelled(None);
        }
        let Some(next) = state.queued.pop_front() else {
            return ActiveQueueResult::Completed {
                delivered: state.delivered,
                deferred_commands: state.deferred_commands,
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
                if crate::repl::commands::is_slash_command(&prompt.text) {
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
