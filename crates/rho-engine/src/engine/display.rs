use super::AgentEngine;
use super::metrics::{StructuralUsage, format_tokens};
use super::tracking::{SessionUsageTotals, UsageTracker};

pub(crate) fn consumed_context_tokens(usage: &StructuralUsage) -> u64 {
    usage
        .input_tokens
        .saturating_add(usage.cached_input_tokens.unwrap_or(0))
        .saturating_add(usage.cache_creation_input_tokens.unwrap_or(0))
}

impl AgentEngine {
    pub(crate) fn consumed_context(&self, usage: &StructuralUsage) -> u64 {
        consumed_context_tokens(usage)
    }

    pub fn context_percent_f64(&self) -> Option<f64> {
        let usage = self.usage.latest()?;
        if !usage.is_reported() {
            return None;
        }
        let limit = self.context_limit();
        let consumed = self.consumed_context(&usage);
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

    pub fn context_remaining_display(&self) -> String {
        let limit = self.context_limit();
        let percent = match self.usage.latest().filter(|usage| usage.is_reported()) {
            Some(usage) => {
                let remaining = limit.saturating_sub(self.consumed_context(&usage) as usize);
                (remaining as f64 / limit as f64) * 100.0
            }
            None => 100.0,
        };
        let percent_str = if (percent.fract() * 10.0).round() == 0.0 {
            format!("{percent:.0}%")
        } else {
            format!("{percent:.1}%")
        };
        format!("{percent_str} ({})", format_tokens(limit as u64))
    }

    pub fn cache_hit_display(&self) -> Option<String> {
        self.usage.latest().as_ref().and_then(format_cache_hit)
    }
}

pub fn format_cache_hit(usage: &StructuralUsage) -> Option<String> {
    let cached = usage.cached_input_tokens.unwrap_or(0);
    if cached == 0 {
        return None;
    }
    let hit_rate = usage.cache_hit_rate()?;
    if (hit_rate.fract() * 10.0).round() == 0.0 {
        Some(format!("cache hit: {hit_rate:.0}%"))
    } else {
        Some(format!("cache hit: {hit_rate:.1}%"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consumed_context_counts_uncached_cached_and_written_input() {
        let usage = StructuralUsage {
            input_tokens: 5_000,
            output_tokens: 1_000,
            total_tokens: 51_000,
            cached_input_tokens: Some(40_000),
            cache_creation_input_tokens: Some(5_000),
            tool_use_prompt_tokens: None,
            reasoning_tokens: None,
        };
        assert_eq!(consumed_context_tokens(&usage), 50_000);
    }

    #[test]
    fn test_cache_hit_rate_normal_ratio() {
        let usage = StructuralUsage {
            input_tokens: 3_000,
            output_tokens: 500,
            total_tokens: 18_500,
            cached_input_tokens: Some(15_000),
            ..Default::default()
        };
        let rate = usage.cache_hit_rate().unwrap();
        assert!((rate - 83.333).abs() < 0.01);
        assert_eq!(format_cache_hit(&usage), Some("cache hit: 83.3%".to_string()));
    }

    #[test]
    fn test_cache_hit_rate_100_percent() {
        let usage = StructuralUsage {
            input_tokens: 0,
            output_tokens: 200,
            total_tokens: 10_200,
            cached_input_tokens: Some(10_000),
            ..Default::default()
        };
        let rate = usage.cache_hit_rate().unwrap();
        assert_eq!(rate, 100.0);
        assert_eq!(format_cache_hit(&usage), Some("cache hit: 100%".to_string()));
    }

    #[test]
    fn test_cache_hit_rate_0_percent() {
        let usage = StructuralUsage {
            input_tokens: 5_000,
            output_tokens: 100,
            total_tokens: 5_100,
            cached_input_tokens: Some(0),
            ..Default::default()
        };
        let rate = usage.cache_hit_rate().unwrap();
        assert_eq!(rate, 0.0);
        assert_eq!(format_cache_hit(&usage), None);
    }

    #[test]
    fn test_cache_hit_rate_zero_token_edge_cases() {
        let empty_cached = StructuralUsage {
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: 0,
            cached_input_tokens: Some(0),
            ..Default::default()
        };
        assert_eq!(empty_cached.cache_hit_rate(), None);
        assert_eq!(format_cache_hit(&empty_cached), None);

        let none_cached = StructuralUsage {
            input_tokens: 5_000,
            output_tokens: 100,
            total_tokens: 5_100,
            cached_input_tokens: None,
            ..Default::default()
        };
        assert_eq!(none_cached.cache_hit_rate(), None);
        assert_eq!(format_cache_hit(&none_cached), None);
    }
}
