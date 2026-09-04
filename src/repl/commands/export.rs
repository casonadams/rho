use super::{CommandResult, SlashCommandContext};
use rho_harness_core::error::Result;

pub async fn handle_export(ctx: &mut SlashCommandContext<'_>, parts: &[&str]) -> Result<Option<CommandResult>> {
    let Some(session_id) = ctx.session_id else {
        ctx.renderer.print_notice("  [Export requires an active session]\n");
        return Ok(Some(CommandResult::Continue));
    };
    let Some(session_manager) = ctx.session_manager else {
        ctx.renderer.print_notice("  [Export requires an active session]\n");
        return Ok(Some(CommandResult::Continue));
    };

    let usage = "Usage: /export [html|md] [path]\n";
    let first = parts.get(1).copied();
    let (extension, path_override) = match first {
        None => ("md", None),
        Some(arg) => {
            let lower = arg.to_ascii_lowercase();
            match lower.as_str() {
                "html" => ("html", parts.get(2).copied()),
                "md" | "markdown" => ("md", parts.get(2).copied()),
                other if other.ends_with(".md") => ("md", Some(arg)),
                other if other.ends_with(".html") || other.ends_with(".htm") => ("html", Some(arg)),
                other if other.contains('/') => ("md", Some(arg)),
                _ => {
                    ctx.renderer.print_notice(usage);
                    return Ok(Some(CommandResult::Continue));
                }
            }
        }
    };

    let tree = session_manager.load_tree().await?;
    let content = match extension {
        "html" => rho_harness_core::session::export::render_html(&tree, session_id),
        _ => rho_harness_core::session::export::render_markdown(&tree, session_id),
    };
    let path = path_override
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| ctx.config.sessions_dir.join(format!("{session_id}.{extension}")));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, content)?;
    ctx.renderer
        .print_notice(&format!("  [Exported session to {}]\n", path.display()));
    Ok(Some(CommandResult::Continue))
}
