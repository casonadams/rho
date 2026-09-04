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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TurnUsage {
    pub totals: StructuralUsage,
    pub active_context: StructuralUsage,
}

impl TurnUsage {
    pub fn new(totals: StructuralUsage, active_context: StructuralUsage) -> Self {
        Self { totals, active_context }
    }

    pub fn single(usage: StructuralUsage) -> Self {
        Self {
            totals: usage,
            active_context: usage,
        }
    }
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

    pub fn record_generation(&mut self, output_tokens: u64, elapsed_ms: u64) {
        self.started_at = None;
        if output_tokens > 0 && elapsed_ms > 0 {
            self.total_output_tokens += output_tokens;
            self.total_elapsed_ms += elapsed_ms;
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

    pub fn record_generation(&self, output_tokens: u64, elapsed_ms: u64) {
        if let Ok(mut speed) = self.speed.lock() {
            speed.record_generation(output_tokens, elapsed_ms);
        }
    }

    pub fn record_turn(&self, usage: TurnUsage, elapsed_ms: u64) {
        if let Ok(mut latest) = self.latest.lock() {
            *latest = usage.active_context.has_values().then_some(usage.active_context);
        }
        if let Ok(mut totals) = self.totals.lock() {
            totals.total_input += usage.totals.input_tokens;
            totals.total_output += usage.totals.output_tokens;
            totals.total_cache_read += usage.totals.cached_input_tokens.unwrap_or(0);
            totals.total_cache_write += usage.totals.cache_creation_input_tokens.unwrap_or(0);
            totals.total_reasoning += usage.totals.reasoning_tokens.unwrap_or(0);
        }
        if elapsed_ms > 0 {
            self.record_generation(usage.totals.output_tokens, elapsed_ms);
        } else {
            self.end_response(usage.totals.output_tokens);
        }
    }

    pub fn record_with_duration(&self, usage: StructuralUsage, elapsed_ms: u64) {
        self.record_turn(TurnUsage::single(usage), elapsed_ms);
    }

    pub fn record(&self, usage: StructuralUsage) {
        self.record_turn(TurnUsage::single(usage), 0);
    }

    pub fn latest(&self) -> Option<StructuralUsage> {
        self.latest.lock().ok().and_then(|usage| *usage)
    }

    pub fn totals(&self) -> SessionUsageTotals {
        self.totals.lock().ok().as_deref().copied().unwrap_or_default()
    }

    pub fn tokens_per_second(&self) -> Option<f64> {
        self.speed.lock().ok().and_then(|s| s.tokens_per_second())
    }

    pub fn reset_speed(&self) {
        if let Ok(mut speed) = self.speed.lock() {
            speed.reset();
        }
    }
}
