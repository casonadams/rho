use std::fmt::Write as _;
use std::io::IsTerminal as _;

use super::types::{CommandResult, SlashCommandContext};
use crate::ui::TerminalRenderer;
use rho_harness_core::error::Result;
use rho_harness_core::skills::{ResolvedSkill, SkillMetadata};

fn print_available_skills(renderer: &TerminalRenderer, skills: &[ResolvedSkill], not_found: Option<&str>) {
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
    if choices.is_empty() {
        return None;
    }
    println!("\nSelect a skill to inspect:");
    for (i, c) in choices.iter().enumerate() {
        println!("  {}. {c}", i + 1);
    }
    use std::io::Write;
    print!("Enter choice [1-{}]: ", choices.len());
    std::io::stdout().flush().ok()?;
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).ok()?;
    let idx = input.trim().parse::<usize>().ok()?.checked_sub(1)?;
    choices
        .get(idx)
        .and_then(|c| c.split_whitespace().next().map(str::to_string))
}

async fn inspect_selected_skill(renderer: &TerminalRenderer, skills: &[ResolvedSkill], name: &str) {
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

pub(crate) async fn handle_skill(ctx: &mut SlashCommandContext<'_>, parts: &[&str]) -> Result<Option<CommandResult>> {
    let cwd = std::env::current_dir().ok();
    let skills = rho_harness_core::skills::resolved_skills_with_home_async(cwd.as_deref(), ctx.home_dir).await;
    let selected = resolve_skill_target(parts, ctx.renderer.has_interactive_ui(), &skills).await;

    match selected.as_deref() {
        Some(name) => inspect_selected_skill(ctx.renderer, &skills, name).await,
        None => print_available_skills(ctx.renderer, &skills, None),
    }
    Ok(Some(CommandResult::Continue))
}

fn format_skill_prompt(meta: &SkillMetadata, content: &str, user_args: &str) -> String {
    if user_args.is_empty() {
        format!(
            "<skill name=\"{}\" location=\"{}\">\n{content}\n</skill>",
            meta.name, meta.location
        )
    } else {
        format!(
            "<skill name=\"{}\" location=\"{}\">\n{content}\n</skill>\n\nSkill input: {user_args}",
            meta.name, meta.location
        )
    }
}

async fn try_handle_skill(
    ctx: &mut SlashCommandContext<'_>,
    skill_name: &str,
    parts: &[&str],
) -> Result<Option<CommandResult>> {
    let cwd = std::env::current_dir().ok();
    let skills = rho_harness_core::skills::resolved_skills_with_home_async(cwd.as_deref(), ctx.home_dir).await;
    let Some(matched) = skills.iter().find(|s| s.metadata.name == skill_name) else {
        return Ok(None);
    };
    let Ok(content) = tokio::fs::read_to_string(&matched.metadata.location).await else {
        return Ok(None);
    };
    ctx.renderer.print_notice(&format!(
        "\n[skill: {} ({})]\n{content}\n",
        matched.metadata.name, matched.origin
    ));
    let text = format_skill_prompt(&matched.metadata, &content, &parts[1..].join(" "));
    Ok(Some(CommandResult::ExpandedPrompt { text }))
}

async fn try_handle_template(ctx: &SlashCommandContext<'_>, custom: &str, parts: &[&str]) -> Option<CommandResult> {
    let cwd = std::env::current_dir().ok();
    let templates =
        rho_harness_core::prompts::discover_prompt_templates_async(Some(&ctx.config.config_dir), cwd.as_deref()).await;
    let template = templates.iter().find(|t| t.metadata.name == custom)?;
    Some(CommandResult::ExpandedPrompt {
        text: template.expand(&parts[1..]),
    })
}

pub(crate) async fn handle_custom(
    ctx: &mut SlashCommandContext<'_>,
    custom: &str,
    parts: &[&str],
) -> Result<Option<CommandResult>> {
    if let Some(skill_name) = custom.strip_prefix("skill:")
        && let Some(res) = try_handle_skill(ctx, skill_name, parts).await?
    {
        return Ok(Some(res));
    }
    if let Some(res) = try_handle_template(ctx, custom, parts).await {
        return Ok(Some(res));
    }

    ctx.renderer.print_notice(&format!(
        "  Unknown command: /{custom}. Type /help for available commands.\n"
    ));
    Ok(Some(CommandResult::Continue))
}
