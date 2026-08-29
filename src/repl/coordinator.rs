use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::engine::AgentEngine;
use crate::engine::provider::host_loop::{CancellationSignal, SteeringQueueProvider};
use crate::engine::runner::{PendingMessageQueue, QUEUED_MESSAGE_BOUNDARY, QueuedMessageBoundary, TurnRequest};
use crate::error::AppError;
use crate::ui::TerminalRenderer;
use crate::ui::interactive::{QueueKind, QueuedMessage};

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
                self.renderer,
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
                                if prompt.kind == QueueKind::Steering {
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
            CoordinatorInput::Prompt(prompt) => queued.push_back(prompt),
            CoordinatorInput::Command(command) => deferred_commands.push(command),
            CoordinatorInput::Cancel => cancelled = true,
        }
    }
    cancelled
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use tokio::sync::mpsc;

    use super::{ActivePromptRunner, ActiveQueueResult, CoordinatorInput, run_active_queue};
    use crate::ui::interactive::{QueueKind, QueuedMessage};

    type PermitSender = mpsc::UnboundedSender<Result<(), &'static str>>;
    type StartedReceiver = mpsc::UnboundedReceiver<String>;
    type Timeline = Arc<Mutex<Vec<String>>>;

    struct FakeRunner {
        permits: tokio::sync::Mutex<mpsc::UnboundedReceiver<Result<(), &'static str>>>,
        started: mpsc::UnboundedSender<String>,
        timeline: Timeline,
    }

    #[async_trait::async_trait]
    impl ActivePromptRunner for FakeRunner {
        type Error = &'static str;

        async fn run_prompt(&self, prompt: &QueuedMessage) -> Result<(), Self::Error> {
            self.timeline.lock().unwrap().push(format!("started:{}", prompt.text));
            self.started.send(prompt.text.clone()).unwrap();
            let result = {
                let mut guard = self.permits.lock().await;
                guard.recv().await.unwrap()
            };
            self.timeline.lock().unwrap().push(format!("finished:{}", prompt.text));
            result
        }

        async fn steer(&self, prompt: &QueuedMessage) -> Result<(), Self::Error> {
            self.timeline.lock().unwrap().push(format!("steered:{}", prompt.text));
            Ok(())
        }

        async fn cancel_active(&self) -> Result<(), Self::Error> {
            self.timeline.lock().unwrap().push("cancelled".to_string());
            Ok(())
        }
    }

    fn prompt(text: &str, kind: QueueKind) -> QueuedMessage {
        QueuedMessage {
            text: text.to_string(),
            kind,
        }
    }

    fn fake_runner() -> (FakeRunner, PermitSender, StartedReceiver, Timeline) {
        let (permit_sender, permits) = mpsc::unbounded_channel();
        let (started, started_receiver) = mpsc::unbounded_channel();
        let timeline = Arc::new(Mutex::new(Vec::new()));
        (
            FakeRunner {
                permits: tokio::sync::Mutex::new(permits),
                started,
                timeline: Arc::clone(&timeline),
            },
            permit_sender,
            started_receiver,
            timeline,
        )
    }

    #[tokio::test]
    async fn steering_prompts_are_delivered_mid_run_and_follow_ups_run_after() {
        let (runner, permits, mut started, timeline) = fake_runner();
        let (input_sender, mut input) = mpsc::unbounded_channel();
        let runner_ref = Arc::new(runner);
        let runner_clone = Arc::clone(&runner_ref);
        let task = tokio::spawn(async move {
            run_active_queue(prompt("active", QueueKind::Steering), &mut input, &*runner_clone).await
        });

        assert_eq!(started.recv().await.as_deref(), Some("active"));
        input_sender
            .send(CoordinatorInput::Prompt(prompt("steer", QueueKind::Steering)))
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        input_sender
            .send(CoordinatorInput::Prompt(prompt("follow", QueueKind::FollowUp)))
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        permits.send(Ok(())).unwrap();
        assert_eq!(started.recv().await.as_deref(), Some("follow"));
        permits.send(Ok(())).unwrap();

        let ActiveQueueResult::Completed { delivered, .. } = task.await.unwrap() else {
            panic!("queue should complete");
        };
        assert_eq!(
            delivered,
            [
                prompt("steer", QueueKind::Steering),
                prompt("active", QueueKind::Steering),
                prompt("follow", QueueKind::FollowUp),
            ]
        );
        assert_eq!(
            *timeline.lock().unwrap(),
            [
                "started:active",
                "steered:steer",
                "finished:active",
                "started:follow",
                "finished:follow",
            ]
        );
    }

    #[tokio::test]
    async fn multiple_follow_ups_run_fifo_after_active_run() {
        let (runner, permits, mut started, timeline) = fake_runner();
        let (input_sender, mut input) = mpsc::unbounded_channel();
        let runner_ref = Arc::new(runner);
        let runner_clone = Arc::clone(&runner_ref);
        let task = tokio::spawn(async move {
            run_active_queue(prompt("active", QueueKind::Steering), &mut input, &*runner_clone).await
        });

        assert_eq!(started.recv().await.as_deref(), Some("active"));
        input_sender
            .send(CoordinatorInput::Prompt(prompt("follow1", QueueKind::FollowUp)))
            .unwrap();
        input_sender
            .send(CoordinatorInput::Prompt(prompt("follow2", QueueKind::FollowUp)))
            .unwrap();
        permits.send(Ok(())).unwrap();
        assert_eq!(started.recv().await.as_deref(), Some("follow1"));
        permits.send(Ok(())).unwrap();
        assert_eq!(started.recv().await.as_deref(), Some("follow2"));
        permits.send(Ok(())).unwrap();

        let ActiveQueueResult::Completed { delivered, .. } = task.await.unwrap() else {
            panic!("queue should complete");
        };
        assert_eq!(
            delivered,
            [
                prompt("active", QueueKind::Steering),
                prompt("follow1", QueueKind::FollowUp),
                prompt("follow2", QueueKind::FollowUp),
            ]
        );
        assert_eq!(
            *timeline.lock().unwrap(),
            [
                "started:active",
                "finished:active",
                "started:follow1",
                "finished:follow1",
                "started:follow2",
                "finished:follow2",
            ]
        );
    }

    #[tokio::test]
    async fn failure_restores_prompts_that_have_not_reached_the_runner() {
        let (runner, permits, mut started, _) = fake_runner();
        let (input_sender, mut input) = mpsc::unbounded_channel();
        let runner_ref = Arc::new(runner);
        let runner_clone = Arc::clone(&runner_ref);
        let task = tokio::spawn(async move {
            run_active_queue(prompt("active", QueueKind::Steering), &mut input, &*runner_clone).await
        });

        started.recv().await.unwrap();
        input_sender
            .send(CoordinatorInput::Prompt(prompt("queued", QueueKind::FollowUp)))
            .unwrap();
        permits.send(Err("provider failed")).unwrap();

        assert_eq!(
            task.await.unwrap(),
            ActiveQueueResult::Failed {
                error: "provider failed",
                delivered: Vec::new(),
                restored: vec![prompt("queued", QueueKind::FollowUp)],
                deferred_commands: Vec::new(),
            }
        );
    }

    #[tokio::test]
    async fn cancellation_restores_queue_and_retains_commands_for_idle_execution() {
        let (runner, _permits, mut started, timeline) = fake_runner();
        let (input_sender, mut input) = mpsc::unbounded_channel();
        let runner_ref = Arc::new(runner);
        let runner_clone = Arc::clone(&runner_ref);
        let task = tokio::spawn(async move {
            run_active_queue(prompt("active", QueueKind::Steering), &mut input, &*runner_clone).await
        });

        started.recv().await.unwrap();
        input_sender
            .send(CoordinatorInput::Command("/model next".to_string()))
            .unwrap();
        input_sender
            .send(CoordinatorInput::Prompt(prompt("queued", QueueKind::FollowUp)))
            .unwrap();
        input_sender.send(CoordinatorInput::Cancel).unwrap();

        assert_eq!(
            task.await.unwrap(),
            ActiveQueueResult::Cancelled {
                delivered: Vec::new(),
                restored: vec![prompt("queued", QueueKind::FollowUp)],
                deferred_commands: vec!["/model next".to_string()],
                cancellation_error: None,
            }
        );
        assert_eq!(*timeline.lock().unwrap(), ["started:active", "cancelled"]);
    }
}
