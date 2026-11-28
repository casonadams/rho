#[derive(Debug, Clone, Default)]
pub struct SpeedTracker {
    total_output_tokens: u64,
    total_elapsed_ms: u64,
}

impl SpeedTracker {
    pub fn record_generation(&mut self, output_tokens: u64, elapsed_ms: u64) {
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
}
