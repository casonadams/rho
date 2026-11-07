use crate::engine::metrics::{RunMetrics, StructuralUsage};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RunStatus {
    Completed,
    ContentFiltered,
    Compacted,
}

pub type UsageDetails = StructuralUsage;

#[derive(Clone, Default)]
pub struct CancellationSignal {
    inner: std::sync::Arc<CancellationSignalInner>,
}

#[derive(Default)]
struct CancellationSignalInner {
    cancelled: AtomicBool,
    notify: tokio::sync::Notify,
}

impl CancellationSignal {
    fn set_flag(&self, flag: &AtomicBool) {
        flag.store(true, Ordering::Relaxed);
        self.inner.notify.notify_waiters();
    }

    pub fn cancel(&self) {
        self.set_flag(&self.inner.cancelled);
    }
    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::Relaxed)
    }
    pub async fn wait_cancelled(&self) {
        while !self.is_cancelled() {
            self.inner.notify.notified().await;
        }
    }
}

#[async_trait]
pub trait SteeringQueueProvider: Send + Sync {
    async fn poll_steering(&self) -> Vec<String> {
        Vec::new()
    }
}

static BACKGROUND_NOTIFICATIONS: std::sync::LazyLock<std::sync::Mutex<Vec<String>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(Vec::new()));

static BACKGROUND_NOTIF_TX: std::sync::LazyLock<tokio::sync::broadcast::Sender<String>> =
    std::sync::LazyLock::new(|| {
        let (tx, _) = tokio::sync::broadcast::channel(64);
        tx
    });

pub fn enqueue_background_notification(notification: String) {
    if let Ok(mut lock) = BACKGROUND_NOTIFICATIONS.lock() {
        lock.push(notification.clone());
    }
    let _ = BACKGROUND_NOTIF_TX.send(notification);
}

pub fn drain_background_notifications() -> Vec<String> {
    if let Ok(mut lock) = BACKGROUND_NOTIFICATIONS.lock() {
        std::mem::take(&mut *lock)
    } else {
        Vec::new()
    }
}

pub fn subscribe_background_notifications() -> tokio::sync::broadcast::Receiver<String> {
    BACKGROUND_NOTIF_TX.subscribe()
}

pub struct TurnRequest<'a> {
    pub prompt: &'a str,
    pub cancellation: Option<&'a CancellationSignal>,
    pub steering: Option<std::sync::Arc<dyn SteeringQueueProvider>>,
}

impl<'a> TurnRequest<'a> {
    pub fn new(prompt: &'a str) -> Self {
        Self {
            prompt,
            cancellation: None,
            steering: None,
        }
    }

    pub fn with_cancellation(mut self, cancellation: &'a CancellationSignal) -> Self {
        self.cancellation = Some(cancellation);
        self
    }

    pub fn with_steering(mut self, steering: std::sync::Arc<dyn SteeringQueueProvider>) -> Self {
        self.steering = Some(steering);
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueuedMessageBoundary {
    ActiveRunCompleted,
}

pub const QUEUED_MESSAGE_BOUNDARY: QueuedMessageBoundary = QueuedMessageBoundary::ActiveRunCompleted;

#[derive(Debug)]
pub struct TurnOutput {
    pub final_text: String,
    pub tool_calls_count: usize,
    pub tool_failures_count: usize,
    pub requests: usize,
    pub usage: Option<UsageDetails>,
    pub status: RunStatus,
    pub metrics: RunMetrics,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_cancellation_signal_notify() {
        let sig = CancellationSignal::default();
        let clone = sig.clone();
        let h = tokio::spawn(async move {
            clone.wait_cancelled().await;
            true
        });
        sig.cancel();
        assert!(sig.is_cancelled() && h.await.unwrap());
    }
}
