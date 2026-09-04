use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use crate::engine::runner::{PendingMessageQueue, SteeringQueueProvider};
use crate::ui::interactive::QueuedMessage;

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
