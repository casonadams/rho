pub mod args;
#[cfg(test)]
mod tests;
pub mod types;

use std::fmt::Write as _;
use std::io::IsTerminal as _;
use std::str::FromStr;

use crate::config::Config;
use crate::ui::TerminalRenderer;
use crate::ui::interactive::footer::format_tokens;
use rho_harness_core::error::Result;
use rho_harness_core::provider::ProviderId;
use rho_harness_core::skills::{ResolvedSkill, SkillMetadata};

pub use types::{CommandResult, SLASH_COMMANDS, SlashCommandContext};

pub fn is_slash_command(trimmed: &str) -> bool {
    if !trimmed.starts_with('/') {
        return false;
    }
    let Some(first_word) = trimmed[1..].split_whitespace().next() else {
        return false;
    };
    !first_word.contains('/') && !first_word.contains('\\') && !std::path::Path::new(trimmed).exists()
}

const HELP_REFERENCE: &str = "\nCommands\n\
  /help                       Show this reference\n\
  /settings                   Interactive runtime interface settings\n\
  /model [model] [provider]   Inspect or switch the model\n\
  /resume [id]                Resume a prior session\n\
  /skill [name]               List or inspect skills\n\
  /mcp                        List configured MCP servers\n\
  /session                    Display token capacity and session diagnostics\n\
  /compact [instructions]     Summarize earlier context to free context space\n\
  /tree                       View conversation turn and branch tree\n\
  /fork [id]                  Fork session from turn or node into a new session\n\
  /clone                      Duplicate active branch into a new session\n\
  /name [name]                Assign a human-readable name to the session\n\
  /rewind <turn>              Rewind context to a specific prior turn\n\
  /clear                      Start a new session; preserve history (alias: /new)\n\
  /login [provider]           Add API-key or subscription auth\n\
  /logout [provider]          Remove stored provider auth\n\
  /reload                     Re-read config, skills, and MCP tools; keep history\n\
  /export [html|md] [path]    Export the active branch as a readable artifact\n\
  /remote                     Pair session with web dashboard via Iroh P2P\n\
  /exit                       Exit rho (alias: /quit)\n\
\nShortcuts\n\
  Tab                         Complete slash commands & skill names\n\
  Shift+Tab                   Cycle thinking level\n\
  Escape                      Cancel active execution / operation\n\
  Ctrl+C                      Clear the input prompt\n\
  Ctrl+D                      Exit at an empty prompt\n\
  Ctrl+L                      Select model\n\
  Ctrl+O                      Expand or collapse tool output\n\
  Ctrl+T                      Toggle thinking blocks visibility\n\
\nCurrent session\n";

fn append_session_help(output: &mut String, config: &Config) {
    let _ = writeln!(output, "  Model                       {}", config.model);
    if let Ok(provider) = ProviderId::from_str(&config.provider) {
        let _ = writeln!(output, "  Provider                    {provider}");
        let _ = writeln!(output, "  Auth mode                   {}", provider.auth_mode_label());
    } else {
        let _ = writeln!(output, "  Provider                    {}", config.provider);
    }
    let thinking = config.thinking_level.as_deref().unwrap_or("none");
    let _ = writeln!(output, "  Thinking                    {thinking}");
}

pub fn print_help(config: &Config, renderer: &TerminalRenderer) {
    let mut output = HELP_REFERENCE.to_string();
    append_session_help(&mut output, config);
    renderer.write_output(&output);
}

fn handle_thinking(ctx: &mut SlashCommandContext<'_>, parts: &[&str]) -> Result<Option<CommandResult>> {
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

fn handle_model(ctx: &mut SlashCommandContext<'_>, parts: &[&str]) -> Result<Option<CommandResult>> {
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

fn parse_export_target<'a>(parts: &'a [&'a str]) -> Option<(&'static str, Option<&'a str>)> {
    let arg = match parts.get(1).copied() {
        None => return Some(("md", None)),
        Some(a) => a,
    };
    let lower = arg.to_ascii_lowercase();
    match lower.as_str() {
        "html" => Some(("html", parts.get(2).copied())),
        "md" | "markdown" => Some(("md", parts.get(2).copied())),
        other if other.ends_with(".md") || other.contains('/') => Some(("md", Some(arg))),
        other if other.ends_with(".html") || other.ends_with(".htm") => Some(("html", Some(arg))),
        _ => None,
    }
}

async fn write_export(path: &std::path::Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(path, content).await.map_err(Into::into)
}

async fn handle_export(ctx: &mut SlashCommandContext<'_>, parts: &[&str]) -> Result<Option<CommandResult>> {
    let (Some(session_id), Some(session_manager)) = (ctx.session_id, ctx.session_manager) else {
        ctx.renderer.print_notice("  [Export requires an active session]\n");
        return Ok(Some(CommandResult::Continue));
    };

    let Some((ext, path_override)) = parse_export_target(parts) else {
        ctx.renderer.print_notice("Usage: /export [html|md] [path]\n");
        return Ok(Some(CommandResult::Continue));
    };

    let tree = session_manager.load_tree().await?;
    let content = if ext == "html" {
        rho_harness_core::session::export::render_html(&tree, session_id)
    } else {
        rho_harness_core::session::export::render_markdown(&tree, session_id)
    };

    let path = path_override
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| ctx.config.sessions_dir.join(format!("{session_id}.{ext}")));
    write_export(&path, &content).await?;
    ctx.renderer
        .print_notice(&format!("  [Exported session to {}]\n", path.display()));
    Ok(Some(CommandResult::Continue))
}

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
    tokio::task::spawn_blocking(move || {
        inquire::Select::new("Select a skill to inspect:", choices)
            .prompt()
            .ok()
            .and_then(|choice| choice.split_whitespace().next().map(str::to_string))
    })
    .await
    .unwrap_or(None)
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

async fn handle_skill(ctx: &mut SlashCommandContext<'_>, parts: &[&str]) -> Result<Option<CommandResult>> {
    let cwd = std::env::current_dir().ok();
    let skills = rho_harness_core::skills::resolved_skills_with_home(cwd.as_deref(), ctx.home_dir);
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

async fn handle_custom(
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

fn append_engine_totals(out: &mut String, engine: &rho_engine::engine::AgentEngine) {
    let totals = engine.session_usage_totals();
    if totals.total_input > 0 || totals.total_output > 0 {
        let _ = writeln!(
            out,
            "  Tokens:                      ↑{} ↓{} (cache: R{} W{})",
            format_tokens(totals.total_input),
            format_tokens(totals.total_output),
            format_tokens(totals.total_cache_read),
            format_tokens(totals.total_cache_write),
        );
        if totals.total_reasoning > 0 {
            let _ = writeln!(
                out,
                "  Reasoning Tokens:            {}",
                format_tokens(totals.total_reasoning)
            );
        }
    }
}

fn append_engine_diagnostics(out: &mut String, engine: &rho_engine::engine::AgentEngine, model: &str) {
    if let Some(quota) = engine.quota_display() {
        let _ = writeln!(out, "  Quota:                       {quota}");
    }
    let capacity = engine
        .context_limit()
        .unwrap_or_else(|| rho_harness_core::tokens::context_window_size(model));
    if capacity > 0 {
        let usage_display = engine.context_remaining_display();
        let pct = engine.context_percent_f64().unwrap_or(0.0);
        let _ = writeln!(
            out,
            "  Context Usage:               {usage_display} / {} tokens ({pct:.1}%)",
            format_tokens(capacity as u64)
        );
    }
    append_engine_totals(out, engine);
    if let Some(tps) = engine.tokens_per_second() {
        let _ = writeln!(out, "  Generation Speed:            {tps:.1} t/s");
    }
}

pub fn handle_session(ctx: &SlashCommandContext<'_>) {
    let mut out = String::new();
    let _ = writeln!(out, "\nSession Diagnostics");
    if let Some(id) = ctx.session_id {
        let _ = writeln!(out, "  Session ID:                  {id}");
    }
    let _ = writeln!(out, "  Model:                       {}", ctx.config.model);
    let _ = writeln!(out, "  Provider:                    {}", ctx.config.provider);
    if let Some(ref level) = ctx.config.thinking_level {
        let _ = writeln!(out, "  Thinking Level:              {level}");
    }
    if let Some(engine) = ctx.engine {
        append_engine_diagnostics(&mut out, engine, &ctx.config.model);
    } else {
        let window = rho_harness_core::tokens::context_window_size(&ctx.config.model);
        let _ = writeln!(out, "  Context Capacity:            {window} tokens");
    }
    let _ = writeln!(
        out,
        "  Reserve Threshold:           {} tokens",
        ctx.config.reserve_tokens
    );
    let _ = writeln!(
        out,
        "  Keep Recent Window:          {} tokens",
        ctx.config.keep_recent_tokens
    );
    let _ = writeln!(out, "  Max Turns:                   {}", ctx.config.max_turns);
    let _ = writeln!(out, "  Steering Mode:               {}", ctx.config.steering_mode);
    let _ = writeln!(out, "  Follow-up Mode:              {}", ctx.config.follow_up_mode);
    let _ = writeln!(out);
    ctx.renderer.print_notice(&out);
}

fn handle_tree_slash_commands(name: &str, parts: &[&str], has_ui: bool) -> Option<CommandResult> {
    match name {
        "tree" => Some(if has_ui {
            CommandResult::OpenTreeSelector
        } else {
            CommandResult::Tree
        }),
        "fork" => Some(CommandResult::ForkSession {
            turn_or_node_id: parts.get(1).map(|s| s.to_string()),
        }),
        "clone" => Some(CommandResult::CloneSession),
        "compact" => {
            let instructions = parts.get(1..).filter(|s| !s.is_empty()).map(|s| s.join(" "));
            Some(CommandResult::Compact { instructions })
        }
        _ => None,
    }
}

fn handle_rewind_command(parts: &[&str], renderer: &TerminalRenderer) -> CommandResult {
    match parts.get(1).and_then(|p| p.parse::<usize>().ok()) {
        Some(turn) => CommandResult::Rewind { turn },
        None => {
            renderer.print_notice("  Usage: /rewind <turn_number> (e.g. /rewind 2)\n");
            CommandResult::Continue
        }
    }
}

fn handle_resume_command(parts: &[&str], has_ui: bool, renderer: &TerminalRenderer) -> CommandResult {
    match parts.get(1) {
        Some(id) => CommandResult::ResumeSession {
            session_id: (*id).to_string(),
        },
        None if has_ui => CommandResult::OpenSessionSelector,
        None => {
            renderer.print_notice("  Usage: /resume <session_id>\n");
            CommandResult::Continue
        }
    }
}

fn handle_session_slash_commands(name: &str, parts: &[&str], renderer: &TerminalRenderer) -> Option<CommandResult> {
    match name {
        "rewind" => Some(handle_rewind_command(parts, renderer)),
        "name" => match parts.get(1..) {
            Some(slice) if !slice.is_empty() => Some(CommandResult::NameSession { name: slice.join(" ") }),
            _ => {
                renderer.print_notice("  Usage: /name <session_name>\n");
                Some(CommandResult::Continue)
            }
        },
        "resume" => Some(handle_resume_command(parts, renderer.has_interactive_ui(), renderer)),
        _ => None,
    }
}

fn handle_settings_command(has_ui: bool, renderer: &TerminalRenderer) -> CommandResult {
    if has_ui {
        CommandResult::OpenSettingsSelector
    } else {
        renderer.print_notice("  Settings: thinking effort, thinking blocks, tool outputs\n");
        CommandResult::Continue
    }
}

fn handle_simple_slash_commands(name: &str, ctx: &mut SlashCommandContext<'_>) -> Option<CommandResult> {
    match name {
        "help" => {
            if ctx.renderer.has_interactive_ui() {
                Some(CommandResult::OpenHelpSelector)
            } else {
                print_help(ctx.config, ctx.renderer);
                Some(CommandResult::Continue)
            }
        }
        "clear" | "reset" | "new" => {
            ctx.renderer.print_status("Context cleared");
            Some(CommandResult::ClearContext)
        }
        "session" | "tokens" => {
            handle_session(ctx);
            Some(CommandResult::Continue)
        }
        "settings" => Some(handle_settings_command(ctx.renderer.has_interactive_ui(), ctx.renderer)),
        "remote" => Some(CommandResult::OpenRemoteModal),
        "reload" => Some(CommandResult::Reload),
        "exit" | "quit" => {
            ctx.renderer.print_notice("  Bye!\n");
            Some(CommandResult::Exit)
        }
        _ => None,
    }
}

fn handle_login_cmd(parts: &[&str]) -> CommandResult {
    match parts.get(1) {
        Some(p) => CommandResult::Login {
            provider: Some((*p).to_string()),
        },
        None => CommandResult::OpenLoginSelector,
    }
}

async fn handle_async_slash_commands(
    name: &str,
    parts: &[&str],
    ctx: &mut SlashCommandContext<'_>,
) -> Result<Option<CommandResult>> {
    match name {
        "thinking" | "think" => handle_thinking(ctx, parts),
        "model" => handle_model(ctx, parts),
        "skill" | "skills" => handle_skill(ctx, parts).await,
        "mcp" => {
            if parts.get(1) == Some(&"login") {
                let target = parts.get(2).map(|s| format!("mcp:{s}"));
                Ok(Some(CommandResult::Login { provider: target }))
            } else if ctx.renderer.has_interactive_ui() {
                Ok(Some(CommandResult::OpenMcpSelector))
            } else {
                ctx.renderer.print_notice("  Configure MCP in ~/.config/rho/mcp.json\n");
                Ok(Some(CommandResult::Continue))
            }
        }
        "login" => Ok(Some(handle_login_cmd(parts))),
        "logout" => Ok(Some(CommandResult::Logout {
            provider: parts.get(1).map(|v| (*v).to_string()),
        })),
        "export" => handle_export(ctx, parts).await,
        custom => handle_custom(ctx, custom, parts).await,
    }
}

pub struct SlashCommandHandler;

impl SlashCommandHandler {
    pub async fn handle(input: &str, ctx: &mut SlashCommandContext<'_>) -> Result<Option<CommandResult>> {
        let trimmed = input.trim();
        if !is_slash_command(trimmed) {
            return Ok(None);
        }

        let parts: Vec<&str> = trimmed[1..].split_whitespace().collect();
        let Some(&first) = parts.first() else {
            return Ok(None);
        };
        let cmd = first.to_lowercase();

        if let Some(res) = handle_simple_slash_commands(&cmd, ctx) {
            return Ok(Some(res));
        }
        if let Some(res) = handle_tree_slash_commands(&cmd, &parts, ctx.renderer.has_interactive_ui()) {
            return Ok(Some(res));
        }
        if let Some(res) = handle_session_slash_commands(&cmd, &parts, ctx.renderer) {
            return Ok(Some(res));
        }
        handle_async_slash_commands(&cmd, &parts, ctx).await
    }
}
