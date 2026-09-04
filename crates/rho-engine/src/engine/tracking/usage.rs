use std::sync::{Arc, Mutex};

use super::in_flight::{InFlightGuard, InFlightUsage};
use super::speed::SpeedTracker;
use super::types::{SessionUsageTotals, TurnUsage};
use crate::engine::metrics::StructuralUsage;

#[derive(Clone, Default)]
pub struct UsageTracker {
    latest: Arc<Mutex<Option<StructuralUsage>>>,
    totals: Arc<Mutex<SessionUsageTotals>>,
    speed: Arc<Mutex<SpeedTracker>>,
    in_flight: Arc<Mutex<InFlightUsage>>,
}

impl UsageTracker {
    pub fn start_turn(&self, estimated_prompt_tokens: Option<u64>) {
        if let Ok(mut in_flight) = self.in_flight.lock() {
            in_flight.start_turn(estimated_prompt_tokens);
        }
    }

    pub fn start_step(&self) {
        if let Ok(mut in_flight) = self.in_flight.lock() {
            in_flight.start_step();
        }
    }

    pub fn record_streaming_chunk(&self, tokens: u64) {
        if let Ok(mut in_flight) = self.in_flight.lock() {
            in_flight.record_streaming_chunk(tokens);
        }
    }

    pub fn record_step(&self, usage: StructuralUsage, elapsed_ms: u64) {
        if let Ok(mut in_flight) = self.in_flight.lock() {
            in_flight.record_step(usage, elapsed_ms);
        }
    }

    pub fn in_flight_guard(&self) -> InFlightGuard<'_> {
        InFlightGuard(self)
    }

    pub fn commit_in_flight_partial(&self) {
        let mut in_flight = match self.in_flight.lock() {
            Ok(guard) => guard,
            Err(_) => return,
        };
        if in_flight.turn_step_totals.has_values() {
            if let Ok(mut totals) = self.totals.lock() {
                totals.add_totals(&in_flight.turn_step_totals);
            }
            if in_flight.turn_step_elapsed_ms > 0 {
                self.record_generation(in_flight.turn_step_totals.total_output, in_flight.turn_step_elapsed_ms);
            }
            if let Some(ctx) = in_flight.latest_context
                && let Ok(mut latest) = self.latest.lock()
            {
                *latest = Some(ctx);
            }
        }
        *in_flight = InFlightUsage::default();
    }

    pub fn clear_in_flight(&self) {
        if let Ok(mut in_flight) = self.in_flight.lock() {
            *in_flight = InFlightUsage::default();
        }
    }

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
        self.clear_in_flight();
        if let Ok(mut latest) = self.latest.lock() {
            *latest = usage.active_context.has_values().then_some(usage.active_context);
        }
        if let Ok(mut totals) = self.totals.lock() {
            totals.add_usage(&usage.totals);
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
        if let Ok(in_flight) = self.in_flight.lock()
            && let Some(usage) = in_flight.latest_context
        {
            return Some(usage);
        }
        self.latest.lock().ok().and_then(|usage| *usage)
    }

    pub fn totals(&self) -> SessionUsageTotals {
        let mut totals = self.totals.lock().ok().as_deref().copied().unwrap_or_default();
        let in_flight = self.in_flight.lock().ok().as_deref().cloned().unwrap_or_default();
        totals.add_totals(&in_flight.turn_step_totals);
        totals.total_input = totals
            .total_input
            .saturating_add(in_flight.estimated_input_tokens.unwrap_or(0));
        totals.total_output = totals.total_output.saturating_add(in_flight.streaming_output_tokens);
        totals
    }

    pub fn tokens_per_second(&self) -> Option<f64> {
        if let Ok(in_flight) = self.in_flight.lock()
            && let Some(speed) = in_flight.tokens_per_second()
        {
            return Some(speed);
        }
        self.speed.lock().ok().and_then(|s| s.tokens_per_second())
    }

    pub fn reset_speed(&self) {
        if let Ok(mut speed) = self.speed.lock() {
            speed.reset();
        }
    }
}
