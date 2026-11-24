use std::fmt::Write as _;

use super::registry::{BUILTIN_COMMANDS, MidTurnPolicy, SHORTCUTS, SlashCommandSpec, lookup_builtin};
use super::types::{CommandResult, SlashCommandContext};
use crate::config::Config;
use crate::ui::TerminalRenderer;
use crate::ui::interactive::footer::format_tokens;

fn format_aliases(aliases: &[&str]) -> String {
    aliases
        .iter()
        .map(|alias| format!("/{alias}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn append_command_line(output: &mut String, spec: &SlashCommandSpec) {
    if spec.aliases.is_empty() {
        let _ = writeln!(output, "  {:<28}{}", spec.usage, spec.summary);
    } else {
        let aliases = format_aliases(spec.aliases);
        let _ = writeln!(output, "  {:<28}{} (alias: {aliases})", spec.usage, spec.summary);
    }
    for sub in spec.subcommands {
        let _ = writeln!(output, "  {:<28}{}", sub.usage, sub.summary);
    }
}

fn format_help_reference() -> String {
    let mut output = String::from("\nCommands\n");
    for spec in BUILTIN_COMMANDS {
        append_command_line(&mut output, spec);
    }
    output.push_str("\nShortcuts\n");
    for shortcut in SHORTCUTS {
        let _ = writeln!(output, "  {:<28}{}", shortcut.key, shortcut.summary);
    }
    output.push_str("\nCurrent session\n");
    output
}

fn append_session_help(output: &mut String, config: &Config) {
    let _ = writeln!(output, "  Model                       {}", config.model);
    if let Some(provider) = config.route().connection.provider_id() {
        let _ = writeln!(output, "  Provider                    {}", config.provider);
        let _ = writeln!(output, "  Auth mode                   {}", provider.auth_mode_label());
    } else {
        let _ = writeln!(output, "  Provider                    {}", config.provider);
    }
    let thinking = rho_harness_core::thinking::ThinkingLevel::from_setting(config.thinking_level.as_deref()).as_str();
    let _ = writeln!(output, "  Thinking                    {thinking}");
}

pub fn print_help(config: &Config, renderer: &TerminalRenderer) {
    let mut output = format_help_reference();
    append_session_help(&mut output, config);
    renderer.write_output(&output);
}

pub fn format_command_help(spec: &SlashCommandSpec) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "\n/{}", spec.name);
    let _ = writeln!(out, "  Usage:      {}", spec.usage);
    if !spec.aliases.is_empty() {
        let _ = writeln!(out, "  Aliases:    {}", format_aliases(spec.aliases));
    }
    let availability = match spec.mid_turn {
        MidTurnPolicy::Immediate => "Available while a turn is running",
        MidTurnPolicy::Queue => "Queued until the active turn finishes",
    };
    let _ = writeln!(out, "  Mid-turn:   {availability}");
    let _ = writeln!(out, "  Summary:    {}", spec.summary);
    let _ = writeln!(out, "\n  {}", spec.detail);
    if !spec.subcommands.is_empty() {
        out.push_str("\nSubcommands\n");
        for sub in spec.subcommands {
            let _ = writeln!(out, "  {:<44}{}", sub.usage, sub.summary);
        }
    }
    out.push('\n');
    out
}

pub fn print_command_help(query: &str, renderer: &TerminalRenderer) {
    match lookup_builtin(query) {
        Some(spec) => renderer.print_notice(&format_command_help(spec)),
        None => {
            let clean = query.trim().trim_start_matches('/');
            renderer.print_notice(&format!(
                "  Unknown command: /{clean}. Run /help to list available commands.\n"
            ));
        }
    }
}

pub(crate) fn handle_help_command(parts: &[&str], ctx: &SlashCommandContext<'_>) -> CommandResult {
    if let Some(target) = parts.get(1) {
        print_command_help(target, ctx.renderer);
        return CommandResult::Continue;
    }
    if ctx.renderer.has_interactive_ui() {
        CommandResult::OpenHelpSelector
    } else {
        print_help(ctx.config, ctx.renderer);
        CommandResult::Continue
    }
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

fn append_engine_diagnostics(out: &mut String, engine: &rho_engine::engine::AgentEngine) {
    if let Some(quota) = engine.quota_display() {
        let _ = writeln!(out, "  Quota:                       {quota}");
    }
    let capacity = engine.context_limit();
    if capacity > 0 {
        let usage_display = engine.context_remaining_display();
        let pct = engine.context_percent_f64().unwrap_or(0.0);
        let source = match engine.context_limit_source() {
            rho_engine::engine::tracking::ContextLimitSource::Configured => "configured",
            rho_engine::engine::tracking::ContextLimitSource::Provider => "from provider",
            rho_engine::engine::tracking::ContextLimitSource::Estimated => "estimated",
        };
        let _ = writeln!(
            out,
            "  Context Usage:               {usage_display} / {} tokens ({pct:.1}%, {source})",
            format_tokens(capacity as u64)
        );
    }
    if let Some(error) = engine.context_probe_error() {
        let _ = writeln!(out, "  Context Window Check:        failed: {error}");
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
        append_engine_diagnostics(&mut out, engine);
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
