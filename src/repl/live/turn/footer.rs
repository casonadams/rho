use crate::engine::AgentEngine;
use crate::ui::interactive::{TerminalBackend, TerminalController};

pub(crate) fn sync_turn_footer<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    engine: &AgentEngine,
) -> bool {
    let footer = controller.state_mut().footer_mut();
    let totals = engine.session_usage_totals();
    let tokens_per_second = engine.tokens_per_second();
    let context_percent = engine.context_percent_f64();
    let context_window = engine.context_limit().unwrap_or(0);
    let context = Some(engine.context_remaining_display());

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
    if footer.tokens_per_second.map(f64::to_bits) != tokens_per_second.map(f64::to_bits) {
        footer.tokens_per_second = tokens_per_second;
        changed = true;
    }
    if footer.context_percent.map(f64::to_bits) != context_percent.map(f64::to_bits) {
        footer.context_percent = context_percent;
        changed = true;
    }
    if footer.context_window != context_window {
        footer.context_window = context_window;
        changed = true;
    }
    if footer.context != context {
        footer.context = context;
        changed = true;
    }
    changed
}
