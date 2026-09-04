use super::{CommandResult, SlashCommandContext};
use rho_harness_core::error::Result;
use std::fmt::Write as _;
use std::io::IsTerminal as _;

pub fn handle_skill(ctx: &mut SlashCommandContext<'_>, parts: &[&str]) -> Result<Option<CommandResult>> {
    let cwd = std::env::current_dir().ok();
    let skills = rho_harness_core::skills::resolved_skills_with_home(cwd.as_deref(), ctx.home_dir);

    let print_available = |ctx: &mut SlashCommandContext<'_>, not_found: Option<&str>| {
        let mut output = not_found
            .map(|name| format!("  Skill '{name}' not found. "))
            .unwrap_or_default();
        output.push_str("Available skills:\n");
        for skill in &skills {
            let _ = writeln!(
                output,
                "    - {}: {} ({})",
                skill.metadata.name, skill.metadata.description, skill.origin
            );
        }
        ctx.renderer.print_notice(&output);
    };

    let selected_name = if parts.len() > 1 {
        Some(parts[1].to_string())
    } else if ctx.renderer.has_interactive_ui() && std::io::stdin().is_terminal() {
        let choices: Vec<String> = skills
            .iter()
            .map(|s| format!("{} - {} ({})", s.metadata.name, s.metadata.description, s.origin))
            .collect();
        inquire::Select::new("Select a skill to inspect:", choices)
            .prompt()
            .ok()
            .and_then(|choice| choice.split_whitespace().next().map(str::to_string))
    } else {
        None
    };

    match selected_name {
        Some(name) => match skills.iter().find(|skill| skill.metadata.name == name) {
            Some(matched) => {
                if let Ok(content) = std::fs::read_to_string(&matched.metadata.location) {
                    ctx.renderer.print_notice(&format!(
                        "\n[skill: {} ({})]\n{content}\n",
                        matched.metadata.name, matched.origin
                    ));
                }
            }
            None => print_available(ctx, Some(&name)),
        },
        None => print_available(ctx, None),
    }
    Ok(Some(CommandResult::Continue))
}
