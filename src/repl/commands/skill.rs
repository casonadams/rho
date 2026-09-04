use super::{CommandResult, SlashCommandContext};
use rho_harness_core::error::Result;
use std::fmt::Write as _;
use std::io::IsTerminal as _;

pub fn handle_skill(ctx: &mut SlashCommandContext<'_>, parts: &[&str]) -> Result<Option<CommandResult>> {
    let cwd = std::env::current_dir().ok();
    let skills = crate::skills::resolved_skills(Some(&ctx.config.config_dir), cwd.as_deref());
    let lookup = |name: &str| skills.iter().find(|skill| skill.metadata.name == name).cloned();
    let list = |output: &mut String| {
        for skill in &skills {
            let _ = writeln!(
                output,
                "    - {}: {} ({})",
                skill.metadata.name, skill.metadata.description, skill.origin
            );
        }
    };
    if parts.len() > 1 {
        let Some(matched) = lookup(parts[1]) else {
            let mut output = format!("  Skill '{}' not found. Available skills:\n", parts[1]);
            list(&mut output);
            ctx.renderer.print_notice(&output);
            return Ok(Some(CommandResult::Continue));
        };
        if let Some(content) = crate::skills::resolved_content(&skills, &matched.metadata.name) {
            ctx.renderer.print_notice(&format!(
                "\n[skill: {} ({})]\n{content}\n",
                matched.metadata.name, matched.origin
            ));
        }
    } else if ctx.renderer.has_interactive_ui() && std::io::stdin().is_terminal() {
        let choices: Vec<String> = skills
            .iter()
            .map(|s| format!("{} - {} ({})", s.metadata.name, s.metadata.description, s.origin))
            .collect();
        let selected = match inquire::Select::new("Select a skill to inspect:", choices).prompt() {
            Ok(choice) => Some(choice.split_whitespace().next().unwrap_or("").to_string()),
            Err(_) => None,
        };
        match selected.and_then(|name| lookup(&name)) {
            Some(matched) => {
                if let Some(content) = crate::skills::resolved_content(&skills, &matched.metadata.name) {
                    ctx.renderer.print_notice(&format!(
                        "\n[skill: {} ({})]\n{content}\n",
                        matched.metadata.name, matched.origin
                    ));
                }
            }
            None => {
                let mut output = String::from("Available skills:\n");
                list(&mut output);
                ctx.renderer.print_notice(&output);
            }
        }
    } else {
        let mut output = String::from("Available skills:\n");
        list(&mut output);
        ctx.renderer.print_notice(&output);
    }
    Ok(Some(CommandResult::Continue))
}
