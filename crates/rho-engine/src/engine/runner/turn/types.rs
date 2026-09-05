use crate::engine::metrics::{RunMetrics, StructuralUsage};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RunStatus {
    Completed,
    ContentFiltered,
}

pub type UsageDetails = StructuralUsage;

#[derive(Default)]
pub struct CancellationSignal {
    cancelled: AtomicBool,
    interrupted: AtomicBool,
}

impl CancellationSignal {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
    pub fn interrupt_stream(&self) {
        self.interrupted.store(true, Ordering::Relaxed);
    }
    pub fn is_interrupted(&self) -> bool {
        self.interrupted.load(Ordering::Relaxed)
    }
    pub fn reset_interrupted(&self) {
        self.interrupted.store(false, Ordering::Relaxed);
    }
}

#[async_trait]
pub trait SteeringQueueProvider: Send + Sync {
    async fn poll_steering(&self) -> Vec<String> {
        Vec::new()
    }
}

pub struct TurnRequest<'a> {
    pub prompt: &'a str,
    pub cancellation: Option<&'a CancellationSignal>,
    pub steering: Option<std::sync::Arc<dyn SteeringQueueProvider>>,
    pub model_switch: Option<std::sync::Arc<SharedModelSwitch>>,
}

impl<'a> TurnRequest<'a> {
    pub fn new(prompt: &'a str) -> Self {
        Self {
            prompt,
            cancellation: None,
            steering: None,
            model_switch: None,
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

    pub fn with_model_switch(mut self, model_switch: std::sync::Arc<SharedModelSwitch>) -> Self {
        self.model_switch = Some(model_switch);
        self
    }
}

#[derive(Clone, Default)]
pub struct SharedModelSwitch {
    inner: std::sync::Arc<std::sync::RwLock<Option<ActiveModelSwitch>>>,
}

impl std::fmt::Debug for SharedModelSwitch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedModelSwitch")
            .field("current_model", &self.current_model())
            .field("current_provider", &self.current_provider())
            .finish()
    }
}

#[derive(Clone)]
pub struct ActiveModelSwitch {
    pub model: String,
    pub provider: String,
    pub handle: rig::agent::ModelHandle,
}

impl ActiveModelSwitch {
    pub fn new(model: impl Into<String>, provider: impl Into<String>, handle: rig::agent::ModelHandle) -> Self {
        Self {
            model: model.into(),
            provider: provider.into(),
            handle,
        }
    }
}

impl SharedModelSwitch {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn switch_to(&self, switch: ActiveModelSwitch) {
        if let Ok(mut lock) = self.inner.write() {
            *lock = Some(switch);
        }
    }

    pub fn get_handle(&self) -> Option<rig::agent::ModelHandle> {
        self.inner
            .read()
            .ok()
            .and_then(|lock| lock.as_ref().map(|s| s.handle.clone()))
    }

    pub fn current_model(&self) -> Option<String> {
        self.inner
            .read()
            .ok()
            .and_then(|lock| lock.as_ref().map(|s| s.model.clone()))
    }

    pub fn current_provider(&self) -> Option<String> {
        self.inner
            .read()
            .ok()
            .and_then(|lock| lock.as_ref().map(|s| s.provider.clone()))
    }

    pub fn take_switched(&self) -> Option<(String, String)> {
        self.inner
            .read()
            .ok()
            .and_then(|lock| lock.as_ref().map(|s| (s.model.clone(), s.provider.clone())))
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
