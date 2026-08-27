use rig::completion::Usage;
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
pub struct UsageTracker {
    latest: Arc<Mutex<Option<Usage>>>,
}

impl UsageTracker {
    pub fn record(&self, usage: Usage) {
        if let Ok(mut latest) = self.latest.lock() {
            *latest = usage.has_values().then_some(usage);
        }
    }

    pub fn latest(&self) -> Option<Usage> {
        self.latest.lock().ok().and_then(|usage| *usage)
    }
}

#[derive(Clone, Default)]
pub struct QuotaTracker {
    latest: Arc<Mutex<Option<String>>>,
}

impl QuotaTracker {
    pub fn replace(&self, value: Option<String>) {
        if let Ok(mut latest) = self.latest.lock() {
            *latest = value;
        }
    }

    pub fn latest(&self) -> Option<String> {
        self.latest.lock().ok().and_then(|value| value.clone())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ContextTracker {
    configured_limit: Option<usize>,
}

impl ContextTracker {
    pub fn new(configured_limit: Option<usize>) -> Self {
        Self { configured_limit }
    }

    pub fn limit_for(&self, model: &str) -> Option<usize> {
        if let Some(limit) = self.configured_limit {
            return Some(limit);
        }
        let lower = model.to_ascii_lowercase();
        if lower.contains("gpt-5") || lower.contains("luna") || lower.contains("codex") {
            Some(376_000)
        } else if lower.contains("o1") || lower.contains("o3") {
            Some(200_000)
        } else if lower.contains("gpt-4") || lower.contains("deepseek") {
            Some(128_000)
        } else if lower.contains("claude") || lower.contains("gemini") {
            Some(1_000_000)
        } else {
            None
        }
    }
}
