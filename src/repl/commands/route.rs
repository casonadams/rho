use super::types::{CommandResult, SlashCommandContext};
use rho_harness_core::error::Result;

pub(crate) fn handle_route(ctx: &mut SlashCommandContext<'_>, parts: &[&str]) -> Result<Option<CommandResult>> {
    match parts.get(1).map(|s| s.to_ascii_lowercase()).as_deref() {
        Some("on") | Some("enable") | Some("true") => {
            ctx.config.routing = true;
            ctx.renderer
                .print_notice("  Model routing enabled (/route off to disable)\n");
            Ok(Some(CommandResult::Continue))
        }
        Some("off") | Some("disable") | Some("false") => {
            ctx.config.routing = false;
            ctx.renderer
                .print_notice("  Model routing disabled (/route on to enable)\n");
            Ok(Some(CommandResult::Continue))
        }
        _ => {
            let status = if ctx.config.routing { "enabled" } else { "disabled" };
            let judge = ctx
                .config
                .judge_model()
                .unwrap_or("not configured (defaults to local Ollama/clef-flash)");
            let smol = ctx
                .config
                .smol_model()
                .unwrap_or("not configured (falls back to default)");
            let standard = &ctx.config.model;
            let slow = ctx
                .config
                .slow_model()
                .unwrap_or("not configured (falls back to default)");

            let message = format!(
                "  Model Routing: {}\n  - Decision: {}\n  - Smol:     {}\n  - Standard: {}\n  - Slow:     {}\n\n  Usage: /route [on|off]\n",
                status, judge, smol, standard, slow
            );
            ctx.renderer.print_notice(&message);
            Ok(Some(CommandResult::Continue))
        }
    }
}
