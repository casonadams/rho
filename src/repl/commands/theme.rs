use super::{CommandResult, SlashCommandContext};
use crate::ui::theme::ThemeRegistry;
use rho_harness_core::error::Result;
use std::io::IsTerminal;

fn switch_theme_by_name(
    ctx: &mut SlashCommandContext<'_>,
    registry: &ThemeRegistry,
    theme_name: &str,
) -> CommandResult {
    if registry.contains(theme_name) {
        return CommandResult::ThemeChanged {
            theme: theme_name.to_string(),
        };
    }

    let mut available = registry.list().iter().map(|t| t.name.as_str()).collect::<Vec<_>>();
    available.sort();
    let list_str = available.join(", ");
    ctx.renderer.print_notice(&format!(
        "  Unknown theme \"{theme_name}\". Available themes:\n  {list_str}\n"
    ));
    CommandResult::Continue
}

fn select_terminal_theme(ctx: &mut SlashCommandContext<'_>, registry: &ThemeRegistry) -> Option<CommandResult> {
    let themes = registry.list();
    let choices: Vec<String> = themes
        .iter()
        .map(|t| {
            let active = if t.name == ctx.config.theme { " (active)" } else { "" };
            format!("{}{active} - {}", t.name, t.description)
        })
        .collect();

    if std::io::stdin().is_terminal()
        && let Ok(choice) = inquire::Select::new("Select a theme:", choices).prompt()
    {
        let selected_name = choice.split_whitespace().next().unwrap_or("default");
        return Some(CommandResult::ThemeChanged {
            theme: selected_name.to_string(),
        });
    }

    ctx.renderer.print_notice("  Available themes:\n");
    for t in themes {
        let active = if t.name == ctx.config.theme { "* " } else { "  " };
        ctx.renderer
            .print_notice(&format!("{active}{} - {}\n", t.name, t.description));
    }
    None
}

pub fn handle_theme(ctx: &mut SlashCommandContext<'_>, parts: &[&str]) -> Result<Option<CommandResult>> {
    let registry = ThemeRegistry::new(Some(&ctx.config.config_dir));

    if parts.len() > 1 {
        return Ok(Some(switch_theme_by_name(ctx, &registry, parts[1].trim())));
    }
    if ctx.renderer.has_interactive_ui() {
        return Ok(Some(CommandResult::OpenThemeSelector));
    }
    Ok(Some(
        select_terminal_theme(ctx, &registry).unwrap_or(CommandResult::Continue),
    ))
}
