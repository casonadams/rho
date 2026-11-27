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
    pub fn cancel(&self) {
        self.inner.cancelled.store(true, Ordering::Release);
        self.inner.notify.notify_waiters();
    }
    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::Acquire)
    }
    pub async fn wait_cancelled(&self) {
        crate::wake::until(&self.inner.notify, || self.is_cancelled().then_some(())).await;
    }

    /// Whether `other` is this same signal rather than one with the same state.
    pub(crate) fn same(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.inner, &other.inner)
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

    /// Returns once steering is waiting; a provider that cannot signal arrivals never returns.
    async fn arrived(&self) {
        std::future::pending::<()>().await;
    }
}

/// Whether a message from the agent's mailbox or the user's steering waits for its next model call.
pub(crate) fn message_waiting(inbox: &crate::mailbox::Mailbox, steering: Option<&dyn SteeringQueueProvider>) -> bool {
    inbox.pending() > 0 || steering.is_some_and(SteeringQueueProvider::has_steering)
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

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_cancel_racing_the_waiter_is_never_lost() {
        for _ in 0..2_000 {
            let sig = CancellationSignal::default();
            let waiter = {
                let sig = sig.clone();
                tokio::spawn(async move { sig.wait_cancelled().await })
            };
            let canceller = tokio::spawn(async move { sig.cancel() });
            canceller.await.unwrap();
            tokio::time::timeout(std::time::Duration::from_secs(2), waiter)
                .await
                .expect("waiter woke after cancel")
                .unwrap();
        }
    }

    #[tokio::test]
    async fn waiting_after_cancel_returns_immediately_and_unless_cancelled_yields_none() {
        let sig = CancellationSignal::default();
        sig.cancel();
        sig.wait_cancelled().await;
        let out = unless_cancelled(Some(&sig), std::future::pending::<()>()).await;
        assert!(out.is_none());
        assert_eq!(unless_cancelled(None, async { 7 }).await, Some(7));
    }
}
