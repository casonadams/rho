use std::time::Instant;

use super::SessionUsageTotals;
use crate::engine::metrics::StructuralUsage;

#[derive(Debug, Clone, Default)]
pub struct InFlightUsage {
    pub turn_step_totals: SessionUsageTotals,
    pub turn_step_elapsed_ms: u64,
    pub estimated_input_tokens: Option<u64>,
    pub streaming_output_tokens: u64,
    pub step_start: Option<Instant>,
    pub latest_context: Option<StructuralUsage>,
}

impl InFlightUsage {
    pub fn start_turn(&mut self, estimated_prompt_tokens: Option<u64>) {
        *self = Self {
            estimated_input_tokens: estimated_prompt_tokens,
            latest_context: estimated_prompt_tokens.map(|tokens| StructuralUsage {
                input_tokens: tokens,
                output_tokens: 0,
                total_tokens: tokens,
                cached_input_tokens: None,
                cache_creation_input_tokens: None,
                tool_use_prompt_tokens: None,
                reasoning_tokens: None,
            }),
            step_start: Some(Instant::now()),
            ..Self::default()
        };
    }

    pub fn start_step(&mut self) {
        if self.step_start.is_none() {
            self.step_start = Some(Instant::now());
        }
    }

    pub fn record_streaming_chunk(&mut self, tokens: u64) {
        if tokens == 0 {
            return;
        }
        self.streaming_output_tokens = self.streaming_output_tokens.saturating_add(tokens);
        if self.step_start.is_none() {
            self.step_start = Some(Instant::now());
        }
    }

    pub fn record_step(&mut self, usage: StructuralUsage, elapsed_ms: u64) {
        self.turn_step_elapsed_ms = self.turn_step_elapsed_ms.saturating_add(elapsed_ms);
        self.step_start = None;
        self.estimated_input_tokens = None;
        if usage.has_values() {
            self.streaming_output_tokens = 0;
            self.turn_step_totals.add_usage(&usage);
            self.latest_context = Some(usage);
        } else {
            self.turn_step_totals.total_output = self
                .turn_step_totals
                .total_output
                .saturating_add(self.streaming_output_tokens);
            self.streaming_output_tokens = 0;
        }
    }

    pub fn tokens_per_second(&self) -> Option<f64> {
        let in_flight_elapsed = self
            .step_start
            .map(|start| start.elapsed().as_millis() as u64)
            .unwrap_or(0);
        let total_elapsed = self.turn_step_elapsed_ms.saturating_add(in_flight_elapsed);
        let total_output = self
            .turn_step_totals
            .total_output
            .saturating_add(self.streaming_output_tokens);

        if total_output == 0 || total_elapsed < 100 {
            return None;
        }
        Some((total_output as f64 / total_elapsed as f64) * 1000.0)
    }
}

pub struct InFlightGuard<'a>(pub(crate) &'a super::UsageTracker);

impl Drop for InFlightGuard<'_> {
    fn drop(&mut self) {
        self.0.commit_in_flight_partial();
    }
}
