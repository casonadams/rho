use super::AgentEngine;
use super::metrics::format_tokens;
use super::tracking::{SessionUsageTotals, UsageTracker};

impl AgentEngine {
    pub fn context_usage_percent(&self) -> Option<usize> {
        let usage = self.usage.latest()?;
        if !usage.has_values() {
            return None;
        }
        let limit = self.context_limit()?;
        let consumed = usage.input_tokens
            + usage.cached_input_tokens.unwrap_or(0)
            + usage.cache_creation_input_tokens.unwrap_or(0);
        Some(((consumed as usize * 100) / limit).min(100))
    }

    pub fn context_percent_f64(&self) -> Option<f64> {
        let usage = self.usage.latest()?;
        if !usage.has_values() {
            return None;
        }
        let limit = self.context_limit()?;
        let consumed = usage.input_tokens
            + usage.cached_input_tokens.unwrap_or(0)
            + usage.cache_creation_input_tokens.unwrap_or(0);
        Some(((consumed as f64 / limit as f64) * 100.0).clamp(0.0, 100.0))
    }

    pub fn session_usage_totals(&self) -> SessionUsageTotals {
        self.usage.totals()
    }

    pub fn usage(&self) -> &UsageTracker {
        &self.usage
    }

    pub fn tokens_per_second(&self) -> Option<f64> {
        self.usage.tokens_per_second()
    }

    pub fn context_display(&self) -> String {
        self.context_remaining_display()
    }

    pub fn context_remaining_display(&self) -> String {
        let limit = self.context_limit();
        let usage = self.usage.latest();
        match (usage, limit) {
            (Some(usage), Some(limit)) if usage.has_values() => {
                let consumed = usage.input_tokens
                    + usage.cached_input_tokens.unwrap_or(0)
                    + usage.cache_creation_input_tokens.unwrap_or(0);
                let remaining = limit.saturating_sub(consumed as usize);
                let percent = (remaining as f64 / limit as f64) * 100.0;
                let percent_str = if (percent.fract() * 10.0).round() == 0.0 {
                    format!("{percent:.0}%")
                } else {
                    format!("{percent:.1}%")
                };
                format!("{percent_str} ({})", format_tokens(limit as u64))
            }
            (None, Some(limit)) | (Some(_), Some(limit)) => format!("100% ({})", format_tokens(limit as u64)),
            (Some(usage), None) if usage.has_values() => format!("{} tokens", format_tokens(usage.input_tokens)),
            _ => "100%".to_string(),
        }
    }

    pub fn context_usage_display(&self) -> String {
        let Some(usage) = self.usage.latest() else {
            return "usage unavailable".to_string();
        };
        if !usage.has_values() {
            return "usage unavailable".to_string();
        }
        let consumed = usage.input_tokens
            + usage.cached_input_tokens.unwrap_or(0)
            + usage.cache_creation_input_tokens.unwrap_or(0);
        if let Some(limit) = self.context_limit() {
            let percent = ((consumed as usize * 100) / limit).min(100);
            format!(
                "{}/{} ({percent}%)",
                format_tokens(consumed),
                format_tokens(limit as u64)
            )
        } else {
            format!("{} input tokens", format_tokens(consumed))
        }
    }
}
