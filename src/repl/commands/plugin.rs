use super::{CommandResult, SlashCommandContext};
use std::fmt::Write as _;

pub fn handle_plugins(ctx: &SlashCommandContext<'_>) -> Option<CommandResult> {
    let mut out = String::from("\nConfigured MCP Servers & Plugins:\n");
    if ctx.config.mcp.servers.is_empty() && ctx.config.plugins.is_empty() {
        out.push_str("  (none configured)\n");
    } else {
        for (name, server) in &ctx.config.mcp.servers {
            let _ = writeln!(
                out,
                "  - [mcp] {name}: {} (enabled: {})",
                server.command, server.enabled
            );
        }
        for (name, plugin) in &ctx.config.plugins {
            let target = plugin
                .command
                .as_deref()
                .map(|c| c.to_string())
                .unwrap_or_else(|| plugin.path.display().to_string());
            let _ = writeln!(out, "  - [plugin] {name}: {target} (enabled: {})", plugin.enabled);
        }
    }
    ctx.renderer.print_notice(&out);
    Some(CommandResult::Continue)
}
