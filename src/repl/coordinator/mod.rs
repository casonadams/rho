use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::engine::AgentEngine;
use crate::engine::runner::{
    CancellationSignal, PendingMessageQueue, QUEUED_MESSAGE_BOUNDARY, QueuedMessageBoundary, SteeringQueueProvider,
    TurnRequest,
};
use crate::error::AppError;
use crate::ui::TerminalRenderer;
use crate::ui::interactive::{QueueKind, QueuedMessage};

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoordinatorInput {
    Prompt(QueuedMessage),
    Command(String),
    Cancel,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ActiveQueueResult<E> {
    Completed {
        delivered: Vec<QueuedMessage>,
        deferred_commands: Vec<String>,
    },
    Failed {
        error: E,
        delivered: Vec<QueuedMessage>,
        restored: Vec<QueuedMessage>,
        deferred_commands: Vec<String>,
    },
    Cancelled {
        delivered: Vec<QueuedMessage>,
        restored: Vec<QueuedMessage>,
        deferred_commands: Vec<String>,
        cancellation_error: Option<E>,
    },
}

#[async_trait]
pub trait ActivePromptRunner: Send + Sync {
    type Error: Send;

    async fn run_prompt(&self, prompt: &QueuedMessage) -> Result<(), Self::Error>;
    async fn steer(&self, prompt: &QueuedMessage) -> Result<(), Self::Error>;
    async fn cancel_active(&self) -> Result<(), Self::Error>;
}

#[derive(Clone, Default)]
pub struct SharedSteeringQueue {
    queue: Arc<Mutex<PendingMessageQueue<String>>>,
}

impl SharedSteeringQueue {
    pub fn new(mode: crate::engine::runner::QueueMode) -> Self {
        Self {
            queue: Arc::new(Mutex::new(PendingMessageQueue::new(mode))),
        }
    }

    pub fn enqueue(&self, msg: String) {
        self.queue.lock().unwrap().enqueue(msg);
    }
}

#[async_trait]
impl SteeringQueueProvider for SharedSteeringQueue {
    async fn poll_steering(&self) -> Vec<String> {
        self.queue.lock().unwrap().drain()
    }
}

pub struct ReplAgentRunner<'a> {
    engine: &'a AgentEngine,
    renderer: &'a TerminalRenderer,
    cancellation: CancellationSignal,
    steering: SharedSteeringQueue,
}

impl<'a> ReplAgentRunner<'a> {
    pub fn new(engine: &'a AgentEngine, renderer: &'a TerminalRenderer) -> Self {
        Self {
            engine,
            renderer,
            cancellation: CancellationSignal::default(),
            steering: SharedSteeringQueue::new(engine.config.steering_mode),
        }
    }
}

#[async_trait]
impl ActivePromptRunner for ReplAgentRunner<'_> {
    type Error = AppError;

    async fn run_prompt(&self, prompt: &QueuedMessage) -> Result<(), Self::Error> {
        self.engine
            .run_turn(
                TurnRequest {
                    prompt: &prompt.text,
                    cancellation: Some(&self.cancellation),
                    steering: Some(&self.steering),
                },
                std::sync::Arc::new(self.renderer.clone()),
            )
            .await
            .map(|_| ())
    }

    async fn steer(&self, prompt: &QueuedMessage) -> Result<(), Self::Error> {
        self.steering.enqueue(prompt.text.clone());
        self.cancellation.interrupt_stream();
        Ok(())
    }

    async fn cancel_active(&self) -> Result<(), Self::Error> {
        self.cancellation.cancel();
        self.engine.record_cancellation("operator interrupt").await
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
