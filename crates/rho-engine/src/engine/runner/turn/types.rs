use crate::engine::metrics::{RunMetrics, StructuralUsage};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RunStatus {
    Completed,
    ContentFiltered,
    Compacted,
    /// Compacted after this turn saved progress, so its prompt and work so far are already in history.
    CompactedMidTurn,
}

impl RunStatus {
    pub fn compaction_guidance(self) -> Option<&'static str> {
        match self {
            Self::Compacted => Some("Context was compacted. Submit your prompt to proceed with compacted context.\n"),
            Self::CompactedMidTurn => Some(
                "Context was compacted; the turn's work so far was kept. Send a follow-up (e.g. \"continue\") instead of resubmitting the prompt.\n",
            ),
            Self::Completed | Self::ContentFiltered => None,
        }
    }
}

pub type UsageDetails = StructuralUsage;

#[derive(Clone, Default, Debug)]
pub struct CancellationSignal {
    inner: std::sync::Arc<CancellationSignalInner>,
}

#[derive(Default, Debug)]
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

/// Runs `fut` to completion, or resolves to `None` when `signal` fires first; no signal means it always completes.
pub async fn unless_cancelled<T>(
    signal: Option<&CancellationSignal>,
    fut: impl std::future::Future<Output = T>,
) -> Option<T> {
    let Some(signal) = signal else {
        return Some(fut.await);
    };
    tokio::select! {
        value = fut => Some(value),
        () = signal.wait_cancelled() => None,
    }
}

#[async_trait]
pub trait SteeringQueueProvider: Send + Sync {
    async fn poll_steering(&self) -> Vec<String> {
        Vec::new()
    }

    /// Whether steering is waiting, without taking it.
    fn has_steering(&self) -> bool {
        false
    }
}

/// Serializes tests that touch the process-wide background jobs and notifications.
#[cfg(test)]
pub(crate) static BACKGROUND_QUEUE_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

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
