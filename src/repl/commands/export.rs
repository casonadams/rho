use super::types::{CommandResult, SlashCommandContext};
use rho_harness_core::error::Result;
use rho_harness_core::session::export::{ExportFormat, write_export};

pub(crate) fn parse_export_target<'a>(parts: &'a [&'a str]) -> Option<(&'static str, Option<&'a str>)> {
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

pub(crate) async fn handle_export(ctx: &mut SlashCommandContext<'_>, parts: &[&str]) -> Result<Option<CommandResult>> {
    let (Some(session_id), Some(session_manager)) = (ctx.session_id, ctx.session_manager) else {
        ctx.renderer.print_notice("  [Export requires an active session]\n");
        return Ok(Some(CommandResult::Continue));
    };

    let Some((ext, path_override)) = parse_export_target(parts) else {
        ctx.renderer.print_notice("Usage: /export [html|md] [path]\n");
        return Ok(Some(CommandResult::Continue));
    };

    let tree = session_manager.load_tree().await?;
    let path = path_override
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| ctx.config.sessions_dir.join(format!("{session_id}.{ext}")));
    write_export(&path, &tree, session_id, ExportFormat::from_extension(ext)).await?;
    ctx.renderer
        .print_notice(&format!("  [Exported session to {}]\n", path.display()));
    Ok(Some(CommandResult::Continue))
}
