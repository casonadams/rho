use super::{CommandResult, SlashCommandContext};
use rho_harness_core::error::Result;
use std::io::IsTerminal as _;

fn apply_explicit_level(ctx: &mut SlashCommandContext<'_>, level: &str) -> CommandResult {
    let valid = crate::repl::interactive::completion::THINKING_LEVELS
        .iter()
        .any(|(lvl, _)| *lvl == level);
    if !valid {
        ctx.renderer.print_notice(&format!(
            "  Unknown thinking level \"{level}\". Available levels: off, minimal, low, medium, high, xhigh, max\n"
        ));
        return CommandResult::Continue;
    }
    ctx.config.thinking_level = if level == "off" { None } else { Some(level.to_string()) };
    ctx.renderer.print_status(&format!("Thinking level: {level}"));
    CommandResult::ThinkingChanged {
        level: if level == "off" { None } else { Some(level.to_string()) },
    }
}

fn prompt_line_thinking(ctx: &mut SlashCommandContext<'_>) -> Option<CommandResult> {
    if !std::io::stdin().is_terminal() {
        return None;
    }
    let levels: Vec<String> = crate::repl::interactive::completion::THINKING_LEVELS
        .iter()
        .map(|(lvl, desc)| format!("{lvl} - {desc}"))
        .collect();
    let choice = inquire::Select::new("Select thinking level:", levels).prompt().ok()?;
    let selected = choice.split_whitespace().next().unwrap_or("off");
    Some(apply_explicit_level(ctx, selected))
}

pub fn handle_thinking(ctx: &mut SlashCommandContext<'_>, parts: &[&str]) -> Result<Option<CommandResult>> {
    if parts.len() > 1 {
        let level = parts[1].to_lowercase();
        return Ok(Some(apply_explicit_level(ctx, &level)));
    }
    if ctx.renderer.has_interactive_ui() {
        return Ok(Some(CommandResult::OpenSettingsSelector));
    }
    Ok(Some(prompt_line_thinking(ctx).unwrap_or(CommandResult::Continue)))
}
