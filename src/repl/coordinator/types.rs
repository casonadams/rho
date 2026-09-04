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
    consumed: Arc<Mutex<Vec<String>>>,
}

impl SharedSteeringQueue {
    pub fn new(mode: crate::engine::runner::QueueMode) -> Self {
        Self {
            queue: Arc::new(Mutex::new(PendingMessageQueue::new(mode))),
            consumed: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn enqueue(&self, msg: String) {
        self.queue.lock().unwrap().enqueue(msg);
    }

    pub fn clear(&self) {
        self.queue.lock().unwrap().clear();
    }

    pub fn consumed(&self) -> Vec<String> {
        self.consumed.lock().unwrap().clone()
    }

    pub fn take_consumed(&self) -> Vec<String> {
        let mut guard = self.consumed.lock().unwrap();
        std::mem::take(&mut *guard)
    }
}

#[async_trait]
impl SteeringQueueProvider for SharedSteeringQueue {
    async fn poll_steering(&self) -> Vec<String> {
        let drained = self.queue.lock().unwrap().drain();
        if !drained.is_empty() {
            self.consumed.lock().unwrap().extend(drained.clone());
        }
        drained
    }
}
