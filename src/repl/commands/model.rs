use super::{CommandResult, SlashCommandContext};
use rho_harness_core::error::Result;

fn resolve_model_spec(parts: &[&str], current_provider: &str) -> (String, String) {
    let model_spec = parts[1];
    if let Some((p, m)) = model_spec.split_once(':') {
        (p.to_string(), m.to_string())
    } else if parts.len() > 2 {
        (parts[2].to_string(), model_spec.to_string())
    } else {
        (current_provider.to_string(), model_spec.to_string())
    }
}

fn prompt_terminal_model_select(ctx: &mut SlashCommandContext<'_>) -> Option<CommandResult> {
    let discovered = crate::repl::interactive::discover_models(ctx.config, ctx.auth_store);
    let models: Vec<String> = discovered
        .iter()
        .map(|m| format!("{} ({}) - {}", m.id, m.provider, m.description))
        .collect();
    let choice = inquire::Select::new("Select a model:", models).prompt().ok()?;
    let model_str = choice.split_whitespace().next().unwrap_or("");
    let provider_str = choice.split('(').nth(1).and_then(|s| s.split(')').next()).unwrap_or("");
    ctx.config.model = model_str.to_string();
    ctx.config.provider = provider_str.to_string();
    ctx.renderer
        .print_status(&format!("Model: {} ({})", ctx.config.model, ctx.config.provider));
    Some(CommandResult::ModelChanged {
        new_model: model_str.to_string(),
        new_provider: Some(provider_str.to_string()),
    })
}

pub fn handle_model(ctx: &mut SlashCommandContext<'_>, parts: &[&str]) -> Result<Option<CommandResult>> {
    if parts.len() > 1 {
        let (provider, model) = resolve_model_spec(parts, &ctx.config.provider);
        ctx.config.provider = provider.clone();
        ctx.config.model = model.clone();
        ctx.renderer
            .print_status(&format!("Model: {} ({})", ctx.config.model, ctx.config.provider));
        return Ok(Some(CommandResult::ModelChanged {
            new_model: model,
            new_provider: Some(provider),
        }));
    }
    if ctx.renderer.has_interactive_ui() {
        return Ok(Some(CommandResult::OpenModelSelector));
    }
    Ok(Some(
        prompt_terminal_model_select(ctx).unwrap_or(CommandResult::Continue),
    ))
}
