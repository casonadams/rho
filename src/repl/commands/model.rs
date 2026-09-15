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
    if discovered.is_empty() {
        return None;
    }
    println!("Select a model:");
    for (idx, m) in discovered.iter().enumerate() {
        println!("  {}. {} ({}) - {}", idx + 1, m.id, m.provider, m.description);
    }
    use std::io::Write;
    print!("Enter choice (1-{}): ", discovered.len());
    let _ = std::io::stdout().flush();
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).ok()?;
    let idx = input.trim().parse::<usize>().ok()?.checked_sub(1)?;
    let m = discovered.get(idx)?;
    ctx.config.model = m.id.clone();
    ctx.config.provider = m.provider.clone();
    ctx.renderer
        .print_status(&format!("Model: {} ({})", ctx.config.model, ctx.config.provider));
    Some(CommandResult::ModelChanged {
        new_model: m.id.clone(),
        new_provider: Some(m.provider.clone()),
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
