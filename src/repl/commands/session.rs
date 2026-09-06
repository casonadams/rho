use super::SlashCommandContext;
use crate::ui::interactive::footer::format_tokens;
use std::fmt::Write as _;

fn append_engine_totals(out: &mut String, engine: &rho_engine::engine::AgentEngine) {
    let totals = engine.session_usage_totals();
    if totals.total_input > 0 || totals.total_output > 0 {
        let _ = writeln!(
            out,
            "  Tokens:                      ↑{} ↓{} (cache: R{} W{})",
            format_tokens(totals.total_input),
            format_tokens(totals.total_output),
            format_tokens(totals.total_cache_read),
            format_tokens(totals.total_cache_write),
        );
        if totals.total_reasoning > 0 {
            let _ = writeln!(
                out,
                "  Reasoning Tokens:            {}",
                format_tokens(totals.total_reasoning)
            );
        }
    }
}

fn append_engine_diagnostics(out: &mut String, engine: &rho_engine::engine::AgentEngine, model: &str) {
    if let Some(quota) = engine.quota_display() {
        let _ = writeln!(out, "  Quota:                       {quota}");
    }
    let capacity = engine
        .context_limit()
        .unwrap_or_else(|| rho_harness_core::tokens::context_window_size(model));
    if capacity > 0 {
        let usage_display = engine.context_remaining_display();
        let pct = engine.context_percent_f64().unwrap_or(0.0);
        let _ = writeln!(
            out,
            "  Context Usage:               {usage_display} / {} tokens ({pct:.1}%)",
            format_tokens(capacity as u64)
        );
    }
    append_engine_totals(out, engine);
    if let Some(tps) = engine.tokens_per_second() {
        let _ = writeln!(out, "  Generation Speed:            {tps:.1} t/s");
    }
}

fn append_session_config_diagnostics(out: &mut String, config: &crate::config::Config) {
    let _ = writeln!(out, "  Reserve Threshold:           {} tokens", config.reserve_tokens);
    let _ = writeln!(
        out,
        "  Keep Recent Window:          {} tokens",
        config.keep_recent_tokens
    );
    let _ = writeln!(out, "  Max Turns:                   {}", config.max_turns);
    let _ = writeln!(out, "  Steering Mode:               {}", config.steering_mode);
    let _ = writeln!(out, "  Follow-up Mode:              {}", config.follow_up_mode);
    let _ = writeln!(out);
}

pub fn handle_session(ctx: &SlashCommandContext<'_>) {
    let mut out = String::new();
    let _ = writeln!(out, "\nSession Diagnostics");
    if let Some(id) = ctx.session_id {
        let _ = writeln!(out, "  Session ID:                  {id}");
    }
    let _ = writeln!(out, "  Model:                       {}", ctx.config.model);
    let _ = writeln!(out, "  Provider:                    {}", ctx.config.provider);
    if let Some(ref level) = ctx.config.thinking_level {
        let _ = writeln!(out, "  Thinking Level:              {level}");
    }
    if let Some(engine) = ctx.engine {
        append_engine_diagnostics(&mut out, engine, &ctx.config.model);
    } else {
        let window = rho_harness_core::tokens::context_window_size(&ctx.config.model);
        let _ = writeln!(out, "  Context Capacity:            {window} tokens");
    }
    append_session_config_diagnostics(&mut out, ctx.config);
    ctx.renderer.print_notice(&out);
}
