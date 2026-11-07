use std::fmt::Write as _;
use std::str::FromStr;

use super::types::SlashCommandContext;
use crate::config::Config;
use crate::ui::TerminalRenderer;
use crate::ui::interactive::footer::format_tokens;
use rho_harness_core::provider::ProviderId;

const HELP_REFERENCE: &str = "\nCommands\n\
  /help                       Show this reference\n\
  /settings                   Interactive runtime interface settings\n\
  /model [<model>]            Inspect or switch the model (<provider>/<model>)\n\
  /resume [id]                Resume a prior session\n\
  /skill [name]               List or inspect skills\n\
  /mcp                        List configured MCP servers\n\
  /session                    Display token capacity and session diagnostics\n\
  /compact [instructions]     Summarize earlier context to free context space\n\
  /tree                       View conversation turn and branch tree\n\
  /fork [id]                  Fork session from turn or node into a new session\n\
  /clone                      Duplicate active branch into a new session\n\
  /name [name]                Assign a human-readable name to the session\n\
  /rewind <turn>              Rewind context to a specific prior turn\n\
  /clear                      Start a new session; preserve history (alias: /new)\n\
  /login [provider]           Add API-key or subscription auth\n\
  /logout [provider]          Remove stored provider auth\n\
  /reload                     Re-read config, skills, and MCP tools; keep history\n\
  /export [html|md] [path]    Export the active branch as a readable artifact\n\
  /exit                       Exit rho (alias: /quit)\n\
\nShortcuts\n\
  Tab                         Complete slash commands & skill names\n\
  Shift+Tab                   Cycle thinking level\n\
  Escape                      Cancel active execution / operation\n\
  Ctrl+C                      Clear the input prompt\n\
  Ctrl+D                      Exit at an empty prompt\n\
  Ctrl+L                      Select model\n\
  Ctrl+O                      Expand or collapse tool output\n\
  Ctrl+T                      Toggle thinking blocks visibility\n\
\nCurrent session\n";

fn append_session_help(output: &mut String, config: &Config) {
    let _ = writeln!(output, "  Model                       {}", config.model);
    if let Ok(provider) = ProviderId::from_str(&config.provider) {
        let _ = writeln!(output, "  Provider                    {provider}");
        let _ = writeln!(output, "  Auth mode                   {}", provider.auth_mode_label());
    } else {
        let _ = writeln!(output, "  Provider                    {}", config.provider);
    }
    let thinking = config.thinking_level.as_deref().unwrap_or("none");
    let _ = writeln!(output, "  Thinking                    {thinking}");
}

pub fn print_help(config: &Config, renderer: &TerminalRenderer) {
    let mut output = HELP_REFERENCE.to_string();
    append_session_help(&mut output, config);
    renderer.write_output(&output);
}

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
        if let Some(hit_ratio) = totals.cache_hit_rate() {
            let _ = writeln!(out, "  Prompt Cache Hit:            {hit_ratio:.1}%");
        }
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
    if let Some(hit) = engine.cache_hit_display() {
        let _ = writeln!(out, "  Cache Efficiency:            {hit}");
    }
    append_engine_totals(out, engine);
    if let Some(tps) = engine.tokens_per_second() {
        let _ = writeln!(out, "  Generation Speed:            {tps:.1} t/s");
    }
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
    let _ = writeln!(
        out,
        "  Reserve Threshold:           {} tokens",
        ctx.config.reserve_tokens
    );
    let _ = writeln!(
        out,
        "  Keep Recent Window:          {} tokens",
        ctx.config.keep_recent_tokens
    );
    let _ = writeln!(out, "  Max Turns:                   {}", ctx.config.max_turns);
    let _ = writeln!(out, "  Steering Mode:               {}", ctx.config.steering_mode);
    let _ = writeln!(out, "  Follow-up Mode:              {}", ctx.config.follow_up_mode);
    let _ = writeln!(out);
    ctx.renderer.print_notice(&out);
}
