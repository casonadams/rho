use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use crate::engine::runner::{PendingMessageQueue, SteeringQueueProvider};

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

#[cfg(test)]
mod tests {
    use super::SharedSteeringQueue;
    use crate::engine::runner::{QueueMode, SteeringQueueProvider};

    #[tokio::test]
    async fn shared_steering_queue_records_and_drains_consumed_prompts() {
        let queue = SharedSteeringQueue::new(QueueMode::All);
        queue.enqueue("first steer".to_string());
        queue.enqueue("second steer".to_string());

        assert!(queue.consumed().is_empty());

        let polled = queue.poll_steering().await;
        assert_eq!(polled, vec!["first steer", "second steer"]);
        assert_eq!(queue.consumed(), vec!["first steer", "second steer"]);

        let taken = queue.take_consumed();
        assert_eq!(taken, vec!["first steer", "second steer"]);
        assert!(queue.consumed().is_empty());

        queue.enqueue("third steer".to_string());
        queue.clear();
        let polled_after_clear = queue.poll_steering().await;
        assert!(polled_after_clear.is_empty());
    }
}
