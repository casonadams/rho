use super::types::{CommandResult, SlashCommandContext};
use rho_harness_core::error::Result;

pub(crate) fn handle_thinking(ctx: &mut SlashCommandContext<'_>, parts: &[&str]) -> Result<Option<CommandResult>> {
    match parts.get(1) {
        Some(level) => {
            let lvl = (*level).to_string();
            ctx.config.thinking_level = Some(lvl.clone());
            ctx.renderer.print_status(&format!("Thinking level set to {lvl}"));
            Ok(Some(CommandResult::ThinkingChanged { level: Some(lvl) }))
        }
        None => {
            if ctx.renderer.has_interactive_ui() {
                Ok(Some(CommandResult::OpenSettingsSelector))
            } else {
                let current = ctx.config.thinking_level.as_deref().unwrap_or("none");
                ctx.renderer.print_notice(&format!("  Thinking level: {current}\n"));
                Ok(Some(CommandResult::Continue))
            }
        }
    }
}

pub(crate) fn resolve_model_spec(parts: &[&str], current_provider: &str) -> (String, String) {
    let model_spec = parts[1];
    let (provider, model) = if let Some((p, m)) = model_spec.split_once('/') {
        (p.to_string(), m.to_string())
    } else if let Some(p) = parts.get(2) {
        ((*p).to_string(), model_spec.to_string())
    } else {
        (
            rho_harness_core::provider::infer_provider_for_model(model_spec)
                .unwrap_or(current_provider)
                .to_string(),
            model_spec.to_string(),
        )
    };
    (provider, model)
}

pub(crate) fn prompt_terminal_model_select(ctx: &mut SlashCommandContext<'_>) -> Option<CommandResult> {
    let discovered = crate::repl::interactive::discover_models(ctx.config, ctx.auth_store);
    let models: Vec<String> = discovered
        .iter()
        .map(|m| format!("{} ({}) - {}", m.id, m.provider, m.description))
        .collect();
    if models.is_empty() {
        return None;
    }
    println!("\nSelect a model:");
    for (i, m) in models.iter().enumerate() {
        println!("  {}. {m}", i + 1);
    }
    use std::io::Write;
    print!("Enter choice [1-{}]: ", models.len());
    std::io::stdout().flush().ok()?;
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).ok()?;
    let choice_idx = input.trim().parse::<usize>().ok()?.checked_sub(1)?;
    let choice = models.get(choice_idx)?;
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

pub(crate) fn handle_model(ctx: &mut SlashCommandContext<'_>, parts: &[&str]) -> Result<Option<CommandResult>> {
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
