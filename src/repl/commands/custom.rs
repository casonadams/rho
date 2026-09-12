use super::{CommandResult, SlashCommandContext};
use rho_harness_core::error::Result;
use rho_harness_core::skills::SkillMetadata;

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
    let skills = rho_harness_core::skills::resolved_skills_with_home(cwd.as_deref(), ctx.home_dir);
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

fn try_handle_template(ctx: &SlashCommandContext<'_>, custom: &str, parts: &[&str]) -> Option<CommandResult> {
    let cwd = std::env::current_dir().ok();
    let templates = rho_harness_core::prompts::discover_prompt_templates(Some(&ctx.config.config_dir), cwd.as_deref());
    let template = templates.iter().find(|t| t.metadata.name == custom)?;
    Some(CommandResult::ExpandedPrompt {
        text: template.expand(&parts[1..]),
    })
}

pub async fn handle_custom(
    ctx: &mut SlashCommandContext<'_>,
    custom: &str,
    parts: &[&str],
) -> Result<Option<CommandResult>> {
    if let Some(skill_name) = custom.strip_prefix("skill:")
        && let Some(res) = try_handle_skill(ctx, skill_name, parts).await?
    {
        return Ok(Some(res));
    }
    if let Some(res) = try_handle_template(ctx, custom, parts) {
        return Ok(Some(res));
    }

    ctx.renderer.print_notice(&format!(
        "  Unknown command: /{custom}. Type /help for available commands.\n"
    ));
    Ok(Some(CommandResult::Continue))
}
