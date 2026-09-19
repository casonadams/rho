use super::super::types::RpcDaemonContext;
use super::session::extract_chat_messages;
use super::turn::handle_compact_cmd;
use crate::error::Result;
use rho_harness_core::rpc::protocol::{RpcCommand, RpcResponse};

pub(crate) async fn handle_state_command<W: tokio::io::AsyncWrite + Unpin>(
    req_id: Option<String>,
    ctx: &mut RpcDaemonContext<'_, W>,
) -> Result<()> {
    let eng = ctx.engine.read().await;
    let cfg = ctx.config.read().await;
    eng.refresh_quota().await;
    let totals = eng.session_usage_totals();
    let approvals = ctx.pending_approvals.lock().await;
    let status = if !approvals.is_empty() {
        "waiting_approval"
    } else if ctx.active_turn.is_some() {
        "busy"
    } else {
        "idle"
    };

    let raw_msgs = eng.session_manager.load_messages().await.unwrap_or_default();
    let messages = extract_chat_messages(&raw_msgs);
    let active_branch = crate::ui::interactive::footer::path::get_git_branch(&eng.base_dir);

    let data = serde_json::json!({
        "session_id": eng.session_manager.session_id,
        "model": cfg.model,
        "provider": cfg.provider,
        "thinking_level": cfg.thinking_level,
        "status": status,
        "messages": messages,
        "active_workspace": eng.base_dir.display().to_string(),
        "active_branch": active_branch,
        "quota": eng.quota_display(),
        "total_input_tokens": totals.total_input,
        "total_output_tokens": totals.total_output,
        "total_cache_read_tokens": totals.total_cache_read,
        "total_cache_write_tokens": totals.total_cache_write,
        "total_cost": serde_json::Value::Null,
        "context_percent": eng.context_percent_f64(),
        "context_window": eng.context_limit().unwrap_or(0),
        "tokens_per_second": eng.tokens_per_second(),
    });
    ctx.writer
        .write_message(&RpcResponse::success(req_id, "get_state", Some(data)))
        .await
}

pub(crate) fn get_host_name() -> String {
    if let Ok(name) = std::env::var("HOSTNAME") {
        let trimmed = name.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    std::process::Command::new("hostname")
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|name| name.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "localhost".to_string())
}

pub(crate) async fn handle_node_info_cmd<W: tokio::io::AsyncWrite + Unpin>(
    req_id: Option<String>,
    ctx: &mut RpcDaemonContext<'_, W>,
) -> Result<()> {
    let hostname = get_host_name();
    let (active_workspace, active_branch) = {
        let eng = ctx.engine.read().await;
        let ws = eng.base_dir.display().to_string();
        let branch = crate::ui::interactive::footer::path::get_git_branch(&eng.base_dir);
        (Some(ws), branch)
    };
    let status = if ctx.active_turn.is_some() { "busy" } else { "idle" };
    let payload = serde_json::json!({
        "hostname": hostname,
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "version": env!("CARGO_PKG_VERSION"),
        "active_workspace": active_workspace,
        "active_branch": active_branch,
        "status": status,
    });
    ctx.writer
        .write_message(&RpcResponse::success(req_id, "get_node_info", Some(payload)))
        .await?;
    Ok(())
}

pub(crate) async fn handle_set_model_cmd<W: tokio::io::AsyncWrite + Unpin>(
    (model, provider): (String, Option<String>),
    req_id: Option<String>,
    ctx: &mut RpcDaemonContext<'_, W>,
) -> Result<()> {
    let mut cfg = ctx.config.write().await;
    cfg.model = model;
    if let Some(p) = provider {
        cfg.provider = p;
    }
    let auth = ctx.auth_store.read().await.clone();
    let mut eng = ctx.engine.write().await;
    match eng.rebuild(cfg.clone(), auth).await {
        Ok(rebuilt) => {
            *eng = rebuilt;
            ctx.writer
                .write_message(&RpcResponse::success(req_id, "set_model", None))
                .await?;
        }
        Err(e) => {
            ctx.writer
                .write_message(&RpcResponse::failure(req_id, "set_model", &e.to_string()))
                .await?;
        }
    }
    Ok(())
}

pub(crate) async fn handle_set_thinking_cmd<W: tokio::io::AsyncWrite + Unpin>(
    level: String,
    req_id: Option<String>,
    ctx: &mut RpcDaemonContext<'_, W>,
) -> Result<()> {
    let mut cfg = ctx.config.write().await;
    cfg.thinking_level = Some(level.clone());
    let mut eng = ctx.engine.write().await;
    eng.config.thinking_level = Some(level);
    let _ = eng.update_model().await;
    ctx.writer
        .write_message(&RpcResponse::success(req_id, "set_thinking", None))
        .await
}

pub(crate) async fn handle_config_update_cmd<W: tokio::io::AsyncWrite + Unpin>(
    cmd: RpcCommand,
    req_id: Option<String>,
    ctx: &mut RpcDaemonContext<'_, W>,
) -> Result<()> {
    match cmd {
        RpcCommand::Compact { instructions } => {
            let eng = ctx.engine.read().await;
            let res = handle_compact_cmd(&eng, instructions, req_id).await;
            ctx.writer.write_message(&res).await?;
        }
        RpcCommand::SetModel { model, provider } => {
            handle_set_model_cmd((model, provider), req_id, ctx).await?;
        }
        RpcCommand::SetThinking { level } => {
            handle_set_thinking_cmd(level, req_id, ctx).await?;
        }
        _ => {}
    }
    Ok(())
}
