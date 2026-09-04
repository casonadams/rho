use super::{CommandResult, SlashCommandContext};
use rho_harness_core::error::Result;

pub async fn handle_custom(
    ctx: &mut SlashCommandContext<'_>,
    custom: &str,
    parts: &[&str],
) -> Result<Option<CommandResult>> {
    let cwd = std::env::current_dir().ok();
    if let Some(skill_name) = custom.strip_prefix("skill:") {
        let skills = rho_harness_core::skills::resolved_skills_with_home(cwd.as_deref(), ctx.home_dir);
        if let Some(matched) = skills.iter().find(|s| s.metadata.name == skill_name)
            && let Ok(content) = std::fs::read_to_string(&matched.metadata.location)
        {
            ctx.renderer.print_notice(&format!(
                "\n[skill: {} ({})]\n{content}\n",
                matched.metadata.name, matched.origin
            ));
            let user_args = parts[1..].join(" ");
            let effective_prompt = if user_args.is_empty() {
                format!(
                    "<skill name=\"{}\" location=\"{}\">\n{}\n</skill>",
                    matched.metadata.name, matched.metadata.location, content
                )
            } else {
                format!(
                    "<skill name=\"{}\" location=\"{}\">\n{}\n</skill>\n\nSkill input: {}",
                    matched.metadata.name, matched.metadata.location, content, user_args
                )
            };
            return Ok(Some(CommandResult::ExpandedPrompt { text: effective_prompt }));
        }
    }

    let templates = rho_harness_core::prompts::discover_prompt_templates(Some(&ctx.config.config_dir), cwd.as_deref());
    if let Some(template) = templates.iter().find(|t| t.metadata.name == custom) {
        let args = &parts[1..];
        let expanded = template.expand(args);
        return Ok(Some(CommandResult::ExpandedPrompt { text: expanded }));
    }

    let plugin_name = custom.strip_prefix("plugin:").unwrap_or(custom);
    if let Some(plugin_cfg) = ctx.config.plugins.get(plugin_name)
        && plugin_cfg.enabled
    {
        let working_dir = cwd.unwrap_or_default();
        let renderer_arc: std::sync::Arc<dyn rho_harness_core::presentation::presenter::Presenter> =
            std::sync::Arc::new(ctx.renderer.clone());
        let dispatcher = std::sync::Arc::new(rho_engine::plugin::host::HostDispatcher::new(renderer_arc));
        if let Ok(daemon) =
            rho_engine::plugin::daemon::DaemonProcess::spawn(rho_engine::plugin::daemon::DaemonSpawnArgs {
                name: plugin_name,
                config: plugin_cfg,
                working_dir: &working_dir,
                dispatcher,
            })
            .await
        {
            let user_args = parts[1..].join(" ");
            let res = daemon
                .call(
                    "hook/command",
                    serde_json::json!({
                        "name": plugin_name,
                        "args": user_args,
                    }),
                )
                .await;
            if let Ok(resp) = res
                && let Some(result) = resp.result
                && let Some(text) = result
                    .get("output")
                    .or_else(|| result.get("message"))
                    .and_then(|v| v.as_str())
            {
                ctx.renderer.print_notice(&format!("\n{text}\n"));
            }
            return Ok(Some(CommandResult::Continue));
        }
    }

    ctx.renderer.print_notice(&format!(
        "  Unknown command: /{custom}. Type /help for available commands.\n"
    ));
    Ok(Some(CommandResult::Continue))
}
