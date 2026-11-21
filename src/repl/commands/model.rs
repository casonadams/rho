use super::types::{CommandResult, SlashCommandContext};
use rho_harness_core::error::Result;
use rho_harness_core::provider::ModelRoute;
use rho_harness_core::thinking::ThinkingLevel;

pub(crate) fn handle_thinking(ctx: &mut SlashCommandContext<'_>, parts: &[&str]) -> Result<Option<CommandResult>> {
    match parts.get(1) {
        Some(level) => match ThinkingLevel::parse_setting(level) {
            Ok(setting) => {
                let shown = ThinkingLevel::from_setting(setting.as_deref()).as_str();
                ctx.config.thinking_level.clone_from(&setting);
                ctx.renderer.print_status(&format!("Thinking level set to {shown}"));
                Ok(Some(CommandResult::ThinkingChanged { level: setting }))
            }
            Err(err) => {
                ctx.renderer.print_status(&err);
                Ok(Some(CommandResult::Continue))
            }
        },
        None => {
            if ctx.renderer.has_interactive_ui() {
                Ok(Some(CommandResult::OpenSettingsSelector))
            } else {
                let current = ThinkingLevel::from_setting(ctx.config.thinking_level.as_deref()).as_str();
                ctx.renderer.print_notice(&format!("  Thinking level: {current}\n"));
                Ok(Some(CommandResult::Continue))
            }
        }
    }
}

fn parse_model_choice(choice: &str) -> (&str, &str) {
    let model_str = choice.split_whitespace().next().unwrap_or("");
    let provider_str = choice.split('(').nth(1).and_then(|s| s.split(')').next()).unwrap_or("");
    (model_str, provider_str)
}

fn parse_model_selection<'a>(input: &str, models: &'a [String]) -> Option<(&'a str, &'a str)> {
    let choice_idx = input.trim().parse::<usize>().ok()?.checked_sub(1)?;
    let choice = models.get(choice_idx)?;
    Some(parse_model_choice(choice))
}

pub(crate) fn prompt_model_choice_from<R: std::io::BufRead, W: std::io::Write>(
    reader: &mut R,
    writer: &mut W,
    models: &[String],
) -> Option<(String, String)> {
    if models.is_empty() {
        return None;
    }
    writeln!(writer, "\nSelect a model:").ok()?;
    for (i, m) in models.iter().enumerate() {
        writeln!(writer, "  {}. {m}", i + 1).ok()?;
    }
    write!(writer, "Enter choice [1-{}]: ", models.len()).ok()?;
    writer.flush().ok()?;
    let mut input = String::new();
    reader.read_line(&mut input).ok()?;
    let (model, provider) = parse_model_selection(&input, models)?;
    Some((model.to_string(), provider.to_string()))
}

pub(crate) fn prompt_terminal_model_select(ctx: &mut SlashCommandContext<'_>) -> Option<CommandResult> {
    let catalog = ctx
        .engine
        .map(|engine| engine.model_catalog().clone())
        .unwrap_or_default();
    let discovered = crate::repl::interactive::discover_models(ctx.config, &catalog, &ctx.auth_store.stored_logins());
    let models: Vec<String> = discovered
        .iter()
        .map(|m| format!("{} ({}) - {}", m.id, m.provider, m.description))
        .collect();
    let mut stdin = std::io::stdin().lock();
    let mut stdout = std::io::stdout();
    let (model_str, provider_str) = prompt_model_choice_from(&mut stdin, &mut stdout, &models)?;
    ctx.config.model = model_str.clone();
    ctx.config.provider = provider_str.clone();
    ctx.renderer
        .print_status(&format!("Model: {} ({})", ctx.config.model, ctx.config.provider));
    Some(CommandResult::ModelChanged {
        new_model: model_str,
        new_provider: Some(provider_str),
    })
}

pub(crate) fn handle_model(ctx: &mut SlashCommandContext<'_>, parts: &[&str]) -> Result<Option<CommandResult>> {
    if let Some(spec) = parts.get(1) {
        let route = ModelRoute::parse(spec, parts.get(2).copied(), &ctx.config.provider);
        ctx.config.set_route(route);
        ctx.renderer
            .print_status(&format!("Model: {} ({})", ctx.config.model, ctx.config.provider));
        return Ok(Some(CommandResult::ModelChanged {
            new_model: ctx.config.model.clone(),
            new_provider: Some(ctx.config.provider.clone()),
        }));
    }
    if ctx.renderer.has_interactive_ui() {
        return Ok(Some(CommandResult::OpenModelSelector));
    }
    Ok(Some(
        prompt_terminal_model_select(ctx).unwrap_or(CommandResult::Continue),
    ))
}
