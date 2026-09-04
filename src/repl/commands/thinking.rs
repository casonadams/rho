use super::{CommandResult, SlashCommandContext};
use rho_harness_core::error::Result;

pub fn handle_thinking(ctx: &mut SlashCommandContext<'_>, parts: &[&str]) -> Result<Option<CommandResult>> {
    if parts.len() > 1 {
        let level = parts[1].to_lowercase();
        ctx.config.thinking_level = if level == "off" { None } else { Some(level.clone()) };
        let _ = rho_harness_core::state::AppState::set_last_thinking_level(
            ctx.config.config_dir.as_path(),
            ctx.config.thinking_level.as_deref(),
        );
        ctx.renderer.print_status(&format!("Thinking level: {level}"));
    } else {
        let levels: Vec<String> = crate::repl::interactive::completion::THINKING_LEVELS
            .iter()
            .map(|(lvl, desc)| format!("{lvl} - {desc}"))
            .collect();
        if let Ok(choice) = inquire::Select::new("Select thinking level:", levels).prompt() {
            let selected_level = choice.split_whitespace().next().unwrap_or("off");
            ctx.config.thinking_level = if selected_level == "off" {
                None
            } else {
                Some(selected_level.to_string())
            };
            let _ = rho_harness_core::state::AppState::set_last_thinking_level(
                ctx.config.config_dir.as_path(),
                ctx.config.thinking_level.as_deref(),
            );
            ctx.renderer.print_status(&format!("Thinking level: {selected_level}"));
        }
    }
    Ok(Some(CommandResult::Continue))
}
