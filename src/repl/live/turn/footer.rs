use crate::engine::AgentEngine;
use crate::ui::interactive::{TerminalBackend, TerminalController};

fn token_changes_differ(
    footer: &mut crate::ui::interactive::FooterState,
    totals: &crate::engine::SessionUsageTotals,
) -> bool {
    let mut changed = false;
    if footer.total_input_tokens != totals.total_input {
        footer.total_input_tokens = totals.total_input;
        changed = true;
    }
    if footer.total_output_tokens != totals.total_output {
        footer.total_output_tokens = totals.total_output;
        changed = true;
    }
    if footer.total_cache_read_tokens != totals.total_cache_read {
        footer.total_cache_read_tokens = totals.total_cache_read;
        changed = true;
    }
    if footer.total_cache_write_tokens != totals.total_cache_write {
        footer.total_cache_write_tokens = totals.total_cache_write;
        changed = true;
    }
    changed
}

struct LiveMetrics {
    tokens_per_second: Option<f64>,
    context_percent: Option<f64>,
    context_window: usize,
    context: Option<String>,
}

fn metric_changes_differ(footer: &mut crate::ui::interactive::FooterState, m: &LiveMetrics) -> bool {
    let mut changed = false;
    if footer.tokens_per_second.map(f64::to_bits) != m.tokens_per_second.map(f64::to_bits) {
        footer.tokens_per_second = m.tokens_per_second;
        changed = true;
    }
    if footer.context_percent.map(f64::to_bits) != m.context_percent.map(f64::to_bits) {
        footer.context_percent = m.context_percent;
        changed = true;
    }
    if footer.context_window != m.context_window {
        footer.context_window = m.context_window;
        changed = true;
    }
    if footer.context != m.context {
        footer.context = m.context.clone();
        changed = true;
    }
    changed
}

pub(crate) fn sync_turn_footer<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    engine: &AgentEngine,
) -> bool {
    let footer = controller.state_mut().footer_mut();
    let totals = engine.session_usage_totals();
    let tokens_per_second = engine.tokens_per_second();
    let context_percent = engine.context_percent_f64();

    let metrics = LiveMetrics {
        tokens_per_second,
        context_percent,
        context_window: engine.context_limit().unwrap_or(0),
        context: Some(engine.context_remaining_display()),
    };
    let mut changed = token_changes_differ(footer, &totals);
    changed |= metric_changes_differ(footer, &metrics);
    changed
}
