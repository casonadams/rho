use std::time::Instant;

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
