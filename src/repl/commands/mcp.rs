use super::{CommandResult, SlashCommandContext};
use std::fmt::Write as _;

pub fn handle_mcp(ctx: &SlashCommandContext<'_>) -> Option<CommandResult> {
    let mut out = String::from("\nConfigured MCP Servers:\n");
    if ctx.config.mcp.servers.is_empty() {
        out.push_str("  (none configured)\n");
    } else {
        for (name, server) in &ctx.config.mcp.servers {
            let target = server
                .command
                .as_deref()
                .or(server.url.as_deref())
                .unwrap_or("<unspecified>");
            let _ = writeln!(out, "  - {name}: {target} (enabled: {})", server.enabled);
        }
    }
    ctx.renderer.print_notice(&out);
    Some(CommandResult::Continue)
}
