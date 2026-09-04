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
        Some(rho_harness_core::tokens::context_window_size(model))
    }
}
