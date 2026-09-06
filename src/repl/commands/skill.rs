use super::{CommandResult, SlashCommandContext};
use rho_harness_core::error::Result;
use rho_harness_core::skills::ResolvedSkill;
use std::fmt::Write as _;
use std::io::IsTerminal as _;

fn print_available_skills(renderer: &crate::ui::TerminalRenderer, skills: &[ResolvedSkill], not_found: Option<&str>) {
    let mut output = not_found
        .map(|name| format!("  Skill '{name}' not found. "))
        .unwrap_or_default();
    output.push_str("Available skills:\n");
    for skill in skills {
        let _ = writeln!(
            output,
            "    - {}: {} ({})",
            skill.metadata.name, skill.metadata.description, skill.origin
        );
    }
    renderer.print_notice(&output);
}

async fn prompt_skill_selection(skills: &[ResolvedSkill]) -> Option<String> {
    let choices: Vec<String> = skills
        .iter()
        .map(|s| format!("{} - {} ({})", s.metadata.name, s.metadata.description, s.origin))
        .collect();
    tokio::task::spawn_blocking(move || {
        inquire::Select::new("Select a skill to inspect:", choices)
            .prompt()
            .ok()
            .and_then(|choice| choice.split_whitespace().next().map(str::to_string))
    })
    .await
    .unwrap_or(None)
}

async fn inspect_selected_skill(renderer: &crate::ui::TerminalRenderer, skills: &[ResolvedSkill], name: &str) {
    if let Some(matched) = skills.iter().find(|s| s.metadata.name == name)
        && let Ok(content) = tokio::fs::read_to_string(&matched.metadata.location).await
    {
        renderer.print_notice(&format!(
            "\n[skill: {} ({})]\n{content}\n",
            matched.metadata.name, matched.origin
        ));
        return;
    }
    print_available_skills(renderer, skills, Some(name));
}

async fn resolve_skill_target(parts: &[&str], has_ui: bool, skills: &[ResolvedSkill]) -> Option<String> {
    if parts.len() > 1 {
        Some(parts[1].to_string())
    } else if !has_ui && std::io::stdin().is_terminal() {
        prompt_skill_selection(skills).await
    } else {
        None
    }
}

pub async fn handle_skill(ctx: &mut SlashCommandContext<'_>, parts: &[&str]) -> Result<Option<CommandResult>> {
    let cwd = std::env::current_dir().ok();
    let skills = rho_harness_core::skills::resolved_skills_with_home(cwd.as_deref(), ctx.home_dir);
    let selected = resolve_skill_target(parts, ctx.renderer.has_interactive_ui(), &skills).await;

    match selected.as_deref() {
        Some(name) => inspect_selected_skill(ctx.renderer, &skills, name).await,
        None => print_available_skills(ctx.renderer, &skills, None),
    }
    Ok(Some(CommandResult::Continue))
}
