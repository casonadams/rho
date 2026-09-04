use crate::engine::metrics::StructuralUsage;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SessionUsageTotals {
    pub total_input: u64,
    pub total_output: u64,
    pub total_cache_read: u64,
    pub total_cache_write: u64,
    pub total_reasoning: u64,
}

impl SessionUsageTotals {
    pub fn has_values(&self) -> bool {
        self.total_input > 0
            || self.total_output > 0
            || self.total_cache_read > 0
            || self.total_cache_write > 0
            || self.total_reasoning > 0
    }

    pub fn add_usage(&mut self, usage: &StructuralUsage) {
        self.total_input = self.total_input.saturating_add(usage.input_tokens);
        self.total_output = self.total_output.saturating_add(usage.output_tokens);
        self.total_cache_read = self
            .total_cache_read
            .saturating_add(usage.cached_input_tokens.unwrap_or(0));
        self.total_cache_write = self
            .total_cache_write
            .saturating_add(usage.cache_creation_input_tokens.unwrap_or(0));
        self.total_reasoning = self.total_reasoning.saturating_add(usage.reasoning_tokens.unwrap_or(0));
    }

    pub fn add_totals(&mut self, other: &Self) {
        self.total_input = self.total_input.saturating_add(other.total_input);
        self.total_output = self.total_output.saturating_add(other.total_output);
        self.total_cache_read = self.total_cache_read.saturating_add(other.total_cache_read);
        self.total_cache_write = self.total_cache_write.saturating_add(other.total_cache_write);
        self.total_reasoning = self.total_reasoning.saturating_add(other.total_reasoning);
    }
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
