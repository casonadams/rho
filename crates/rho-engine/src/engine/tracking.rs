use crate::engine::metrics::StructuralUsage;
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SessionUsageTotals {
    pub total_input: u64,
    pub total_output: u64,
    pub total_cache_read: u64,
    pub total_cache_write: u64,
    pub total_reasoning: u64,
}

#[derive(Debug, Clone, Default)]
pub struct SpeedTracker {
    started_at: Option<Instant>,
    total_output_tokens: u64,
    total_elapsed_ms: u64,
}

impl SpeedTracker {
    pub fn response_start(&mut self) {
        self.started_at = Some(Instant::now());
    }

    pub fn response_end(&mut self, output_tokens: u64) {
        if let Some(start) = self.started_at.take()
            && output_tokens > 0
        {
            self.total_output_tokens += output_tokens;
            self.total_elapsed_ms += start.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
        }
    }

    pub fn tokens_per_second(&self) -> Option<f64> {
        if self.total_output_tokens == 0 || self.total_elapsed_ms == 0 {
            return None;
        }
        Some((self.total_output_tokens as f64 / self.total_elapsed_ms as f64) * 1000.0)
    }

    pub fn reset(&mut self) {
        self.started_at = None;
        self.total_output_tokens = 0;
        self.total_elapsed_ms = 0;
    }
}

#[derive(Clone, Default)]
pub struct UsageTracker {
    latest: Arc<Mutex<Option<StructuralUsage>>>,
    totals: Arc<Mutex<SessionUsageTotals>>,
    speed: Arc<Mutex<SpeedTracker>>,
}

impl UsageTracker {
    pub fn start_response(&self) {
        if let Ok(mut speed) = self.speed.lock() {
            speed.response_start();
        }
    }

    pub fn end_response(&self, output_tokens: u64) {
        if let Ok(mut speed) = self.speed.lock() {
            speed.response_end(output_tokens);
        }
    }

    pub fn record(&self, usage: StructuralUsage) {
        if let Ok(mut latest) = self.latest.lock() {
            *latest = usage.has_values().then_some(usage);
        }
        if let Ok(mut totals) = self.totals.lock() {
            totals.total_input += usage.input_tokens;
            totals.total_output += usage.output_tokens;
            totals.total_cache_read += usage.cached_input_tokens.unwrap_or(0);
            totals.total_cache_write += usage.cache_creation_input_tokens.unwrap_or(0);
            totals.total_reasoning += usage.reasoning_tokens.unwrap_or(0);
        }
        self.end_response(usage.output_tokens);
    }

    pub fn latest(&self) -> Option<StructuralUsage> {
        self.latest.lock().ok().and_then(|usage| *usage)
    }

    pub fn totals(&self) -> SessionUsageTotals {
        self.totals.lock().ok().map(|g| *g).unwrap_or_default()
    }

    pub fn tokens_per_second(&self) -> Option<f64> {
        self.speed.lock().ok().and_then(|s| s.tokens_per_second())
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
        super::provider::registry::context_limit(model)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_tracker_accumulates_totals_across_turns() {
        let tracker = UsageTracker::default();
        let turn1 = StructuralUsage {
            input_tokens: 100,
            output_tokens: 50,
            total_tokens: 150,
            cached_input_tokens: Some(20),
            cache_creation_input_tokens: Some(10),
            tool_use_prompt_tokens: None,
            reasoning_tokens: Some(5),
        };
        let turn2 = StructuralUsage {
            input_tokens: 200,
            output_tokens: 80,
            total_tokens: 280,
            cached_input_tokens: Some(40),
            cache_creation_input_tokens: None,
            tool_use_prompt_tokens: None,
            reasoning_tokens: None,
        };

        tracker.record(turn1);
        tracker.record(turn2);

        let totals = tracker.totals();
        assert_eq!(totals.total_input, 300);
        assert_eq!(totals.total_output, 130);
        assert_eq!(totals.total_cache_read, 60);
        assert_eq!(totals.total_cache_write, 10);
        assert_eq!(totals.total_reasoning, 5);
        assert_eq!(tracker.latest(), Some(turn2));
    }

    #[test]
    fn speed_tracker_computes_rate_and_resets() {
        let mut speed = SpeedTracker::default();
        speed.response_start();
        std::thread::sleep(std::time::Duration::from_millis(5));
        speed.response_end(100);

        let tps = speed.tokens_per_second();
        assert!(tps.is_some());
        assert!(tps.unwrap() > 0.0);

        speed.reset();
        assert_eq!(speed.tokens_per_second(), None);
    }
}
