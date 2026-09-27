use std::sync::Arc;

use rho_harness_core::collab::{CollabHostConfig, CollabHostServer};
use rho_harness_core::error::Result;

use super::types::{CommandResult, SlashCommandContext};

pub(crate) async fn handle_collab(ctx: &mut SlashCommandContext<'_>, parts: &[&str]) -> Result<Option<CommandResult>> {
    let sub = parts.get(1).copied().unwrap_or("");
    match sub {
        "" | "start" => handle_collab_start(ctx).await,
        "kick" => handle_collab_kick(ctx, parts.get(2).copied()).await,
        "rotate" => handle_collab_rotate(ctx).await,
        "stop" => handle_collab_stop(ctx).await,
        "link" | "links" => handle_collab_link(ctx).await,
        "peers" | "list" => handle_collab_peers(ctx).await,
        _ => {
            ctx.renderer
                .print_notice("  Unknown collab command. Usage: /collab [start|stop|link|peers|kick <id>|rotate]\n");
            Ok(Some(CommandResult::Continue))
        }
    }
}

async fn handle_collab_start(ctx: &mut SlashCommandContext<'_>) -> Result<Option<CommandResult>> {
    let Some(ref mut collab_slot) = ctx.collab else {
        ctx.renderer.print_notice("  Collab is not supported in this session\n");
        return Ok(Some(CommandResult::Continue));
    };

    if collab_slot.is_some() {
        if ctx.renderer.has_interactive_ui() {
            return Ok(Some(CommandResult::OpenCollabSelector));
        }
        ctx.renderer
            .print_notice("  Collab session is already active. Use /collab link or /collab stop\n");
        return Ok(Some(CommandResult::Continue));
    }

    let server = CollabHostServer::start(CollabHostConfig::default()).await?;
    let (full, view) = server.tickets().await?;
    **collab_slot = Some(Arc::new(server));

    let full_link = format!("rho join {}", full.to_uri());
    let view_link = format!("rho join {}", view.to_uri());
    let _ = crate::platform::clipboard::set_text(&full_link);

    ctx.renderer.print_notice(&format!(
        "  ● Collab session active\n    Co-pilot:  {full_link}\n    View-only: {view_link}\n    (Copied co-pilot link to clipboard)\n"
    ));

    Ok(Some(CommandResult::Continue))
}

async fn handle_collab_kick(ctx: &mut SlashCommandContext<'_>, target: Option<&str>) -> Result<Option<CommandResult>> {
    let Some(id) = target else {
        ctx.renderer.print_notice("  Usage: /collab kick <id>\n");
        return Ok(Some(CommandResult::Continue));
    };

    if let Some(ref collab_slot) = ctx.collab
        && let Some(ref server) = **collab_slot
    {
        if server.kick_peer_by_spec(id).await {
            ctx.renderer.print_notice(&format!("  ● Collaborator {id} kicked\n"));
        } else {
            ctx.renderer.print_notice(&format!("  Collaborator {id} not found\n"));
        }
    } else {
        ctx.renderer
            .print_notice("  Collab session is not active. Start one with /collab\n");
    }

    Ok(Some(CommandResult::Continue))
}

async fn handle_collab_rotate(ctx: &mut SlashCommandContext<'_>) -> Result<Option<CommandResult>> {
    if let Some(ref collab_slot) = ctx.collab
        && let Some(ref server) = **collab_slot
    {
        let (full, view) = server.rotate_secret().await?;
        let full_link = format!("rho join {}", full.to_uri());
        let view_link = format!("rho join {}", view.to_uri());
        let _ = crate::platform::clipboard::set_text(&full_link);

        ctx.renderer.print_notice(&format!(
            "  ● Collab keys rotated (all previous peers disconnected)\n    Co-pilot:  {full_link}\n    View-only: {view_link}\n    (Copied co-pilot link to clipboard)\n"
        ));
    } else {
        ctx.renderer
            .print_notice("  Collab session is not active. Start one with /collab\n");
    }

    Ok(Some(CommandResult::Continue))
}

async fn handle_collab_stop(ctx: &mut SlashCommandContext<'_>) -> Result<Option<CommandResult>> {
    if let Some(ref mut collab_slot) = ctx.collab {
        if let Some(server) = collab_slot.take() {
            server.stop().await;
            ctx.renderer.print_notice("  ● Collab session stopped\n");
        } else {
            ctx.renderer.print_notice("  Collab session is not active\n");
        }
    } else {
        ctx.renderer.print_notice("  Collab session is not active\n");
    }

    Ok(Some(CommandResult::Continue))
}

async fn handle_collab_link(ctx: &mut SlashCommandContext<'_>) -> Result<Option<CommandResult>> {
    if let Some(ref collab_slot) = ctx.collab
        && let Some(ref server) = **collab_slot
    {
        let (full, view) = server.tickets().await?;
        let full_link = format!("rho join {}", full.to_uri());
        let view_link = format!("rho join {}", view.to_uri());
        let _ = crate::platform::clipboard::set_text(&full_link);

        ctx.renderer.print_notice(&format!(
            "  ● Active Collab Session\n    Co-pilot:  {full_link}\n    View-only: {view_link}\n    (Copied co-pilot link to clipboard)\n"
        ));
    } else {
        ctx.renderer
            .print_notice("  Collab session is not active. Start one with /collab\n");
    }

    Ok(Some(CommandResult::Continue))
}

pub(crate) fn print_peer_list(
    renderer: &crate::ui::TerminalRenderer,
    peers: &[rho_harness_core::collab::CollabPeerInfo],
) {
    if peers.is_empty() {
        renderer.print_notice("  No active collaborators connected\n");
        return;
    }
    renderer.print_notice(&format!("  Active collaborators ({}):\n", peers.len()));
    for p in peers {
        renderer.print_notice(&format!("    #{}: {:?} (connected {})\n", p.id, p.role, p.connected_at));
    }
}

async fn handle_collab_peers(ctx: &mut SlashCommandContext<'_>) -> Result<Option<CommandResult>> {
    let Some(ref collab_slot) = ctx.collab else {
        ctx.renderer
            .print_notice("  Collab session is not active. Start one with /collab\n");
        return Ok(Some(CommandResult::Continue));
    };
    let Some(ref server) = **collab_slot else {
        ctx.renderer
            .print_notice("  Collab session is not active. Start one with /collab\n");
        return Ok(Some(CommandResult::Continue));
    };

    if ctx.renderer.has_interactive_ui() {
        return Ok(Some(CommandResult::OpenCollabSelector));
    }
    let peers = server.peers().await;
    print_peer_list(ctx.renderer, &peers);
    Ok(Some(CommandResult::Continue))
}
