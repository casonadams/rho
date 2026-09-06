use super::{CommandResult, SlashCommandContext};
use rho_harness_core::error::Result;

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

pub async fn handle_export(ctx: &mut SlashCommandContext<'_>, parts: &[&str]) -> Result<Option<CommandResult>> {
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
