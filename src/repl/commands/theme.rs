use super::{CommandResult, SlashCommandContext};
use crate::ui::theme::ThemeRegistry;
use rho_harness_core::error::Result;
use std::io::IsTerminal;

pub fn handle_theme(ctx: &mut SlashCommandContext<'_>, parts: &[&str]) -> Result<Option<CommandResult>> {
    let registry = ThemeRegistry::new(Some(&ctx.config.config_dir));

    if parts.len() > 1 {
        let theme_name = parts[1].trim();
        if registry.contains(theme_name) {
            return Ok(Some(CommandResult::ThemeChanged {
                theme: theme_name.to_string(),
            }));
        }

        let mut available = registry.list().iter().map(|t| t.name.as_str()).collect::<Vec<_>>();
        available.sort();
        let list_str = available.join(", ");
        ctx.renderer.print_notice(&format!(
            "  Unknown theme \"{theme_name}\". Available themes:\n  {list_str}\n"
        ));
        Ok(Some(CommandResult::Continue))
    } else if ctx.renderer.has_interactive_ui() {
        Ok(Some(CommandResult::OpenThemeSelector))
    } else {
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
            return Ok(Some(CommandResult::ThemeChanged {
                theme: selected_name.to_string(),
            }));
        }

        ctx.renderer.print_notice("  Available themes:\n");
        for t in themes {
            let active = if t.name == ctx.config.theme { "* " } else { "  " };
            ctx.renderer
                .print_notice(&format!("{active}{} - {}\n", t.name, t.description));
        }
        Ok(Some(CommandResult::Continue))
    }
}
