use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use rho_harness_core::locking::lock;
use tokio::sync::Notify;

use crate::engine::runner::{PendingMessageQueue, SteeringQueueProvider};

#[derive(Clone, Default)]
pub struct SharedSteeringQueue {
    queue: Arc<Mutex<PendingMessageQueue<String>>>,
    consumed: Arc<Mutex<Vec<String>>>,
    enqueued: Arc<Notify>,
}

impl SharedSteeringQueue {
    pub fn new(mode: crate::engine::runner::QueueMode) -> Self {
        Self {
            queue: Arc::new(Mutex::new(PendingMessageQueue::new(mode))),
            ..Self::default()
        }
    }

    pub fn enqueue(&self, msg: String) {
        lock(&self.queue).enqueue(msg);
        self.enqueued.notify_waiters();
    }

    pub fn clear(&self) {
        lock(&self.queue).clear();
    }

    pub fn consumed(&self) -> Vec<String> {
        lock(&self.consumed).clone()
    }

    pub fn take_consumed(&self) -> Vec<String> {
        let mut guard = lock(&self.consumed);
        std::mem::take(&mut *guard)
    }
}

#[async_trait]
impl SteeringQueueProvider for SharedSteeringQueue {
    async fn poll_steering(&self) -> Vec<String> {
        let drained = lock(&self.queue).drain();
        if !drained.is_empty() {
            lock(&self.consumed).extend(drained.clone());
        }
        drained
    }

    fn has_steering(&self) -> bool {
        !lock(&self.queue).is_empty()
    }

    async fn arrived(&self) {
        rho_engine::wake::until(&self.enqueued, || self.has_steering().then_some(())).await;
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

    #[tokio::test(start_paused = true)]
    async fn a_waiter_wakes_when_steering_is_enqueued() {
        let queue = SharedSteeringQueue::new(QueueMode::All);
        let waiter = tokio::spawn({
            let queue = queue.clone();
            async move { queue.arrived().await }
        });
        tokio::task::yield_now().await;
        assert!(!waiter.is_finished());

        queue.enqueue("steer".to_string());

        let started = tokio::time::Instant::now();
        waiter.await.unwrap();
        assert_eq!(started.elapsed(), std::time::Duration::ZERO);
    }
}
