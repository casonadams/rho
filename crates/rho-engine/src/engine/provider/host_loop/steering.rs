use async_trait::async_trait;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::Notify;

#[derive(Clone, Default)]
pub struct CancellationSignal {
    cancelled: Arc<AtomicBool>,
    interrupted: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

impl CancellationSignal {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        self.notify.notify_waiters();
    }

    pub fn interrupt_stream(&self) {
        self.interrupted.store(true, Ordering::Release);
        self.notify.notify_waiters();
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    pub fn is_interrupted(&self) -> bool {
        self.interrupted.load(Ordering::Acquire)
    }

    pub fn reset_interrupted(&self) {
        self.interrupted.store(false, Ordering::Release);
    }

    pub(crate) async fn cancelled(&self) {
        while !self.is_cancelled() {
            self.notify.notified().await;
        }
    }

    pub(crate) async fn cancelled_or_interrupted(&self) {
        while !self.is_cancelled() && !self.is_interrupted() {
            self.notify.notified().await;
        }
    }
}

#[async_trait]
pub trait SteeringQueueProvider: Send + Sync {
    async fn poll_steering(&self) -> Vec<String>;
}

pub struct NoopSteeringQueue;

#[async_trait]
impl SteeringQueueProvider for NoopSteeringQueue {
    async fn poll_steering(&self) -> Vec<String> {
        Vec::new()
    }
}
