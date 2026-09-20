use tokio::sync::mpsc;

use super::handlers::{
    handle_abort_cmd, handle_config_update_cmd, handle_prompt_cmd, handle_remote_auth_cmd, handle_resume_or_fork_cmd,
    handle_session_lifecycle_cmd, handle_state_command, handle_steer_command, handle_tool_response_cmd,
};
use super::types::{NextRpcItem, RpcDaemonContext, RpcLoopAction};
use crate::error::Result;
use rho_engine::engine::runner::TurnOutput;
use rho_harness_core::rpc::protocol::{RpcCommand, RpcEvent, RpcRequest, RpcResponse};
use rho_harness_core::rpc::transport::{JsonLinesReader, JsonLinesWriter};

pub(crate) async fn dispatch_rpc<W: tokio::io::AsyncWrite + Unpin>(
    (cmd, req_id): (RpcCommand, Option<String>),
    ctx: &mut RpcDaemonContext<'_, W>,
) -> Result<RpcLoopAction> {
    match cmd {
        RpcCommand::Prompt { message, .. } => {
            handle_prompt_cmd((message, req_id), ctx).await?;
        }
        RpcCommand::Steer { message } => {
            handle_steer_command((message, req_id), ctx).await?;
        }
        RpcCommand::Abort => {
            handle_abort_cmd(req_id, ctx).await?;
        }
        RpcCommand::ToolResponse { approval_id, decision } => {
            handle_tool_response_cmd((approval_id, decision, req_id), ctx).await?;
        }
        RpcCommand::GetState => {
            handle_state_command(req_id, ctx).await?;
        }
        ref other if handle_remote_auth_cmd(other, req_id.clone(), ctx).await? => {}
        RpcCommand::Exit => {
            if let Some(handle) = ctx.active_turn.take() {
                handle.abort();
            }
            ctx.writer
                .write_message(&RpcResponse::success(req_id, "exit", None))
                .await?;
            return Ok(RpcLoopAction::Break);
        }
        ref other if handle_session_lifecycle_cmd(other, req_id.clone(), ctx).await? => {}
        ref other if handle_resume_or_fork_cmd(other, req_id.clone(), ctx).await? => {}
        other => {
            handle_config_update_cmd(other, req_id, ctx).await?;
        }
    }
    Ok(RpcLoopAction::Continue)
}

pub(crate) async fn handle_read_request<W: tokio::io::AsyncWrite + Unpin>(
    res: rho_harness_core::error::Result<Option<RpcRequest>>,
    writer: &mut JsonLinesWriter<W>,
) -> Result<Option<RpcRequest>> {
    match res {
        Ok(opt) => Ok(opt),
        Err(e) => {
            writer
                .write_message(&RpcResponse::failure(None, "parse", &e.to_string()))
                .await?;
            Ok(None)
        }
    }
}

pub(crate) async fn poll_next_rpc<R: tokio::io::AsyncBufRead + Unpin>(
    reader: &mut JsonLinesReader<R>,
    event_rx: &mut mpsc::UnboundedReceiver<RpcEvent>,
    active_turn: &mut Option<tokio::task::JoinHandle<Result<TurnOutput>>>,
) -> NextRpcItem {
    if let Some(turn) = active_turn.as_mut() {
        tokio::select! {
            ev = event_rx.recv() => NextRpcItem::Event(ev),
            res = turn => NextRpcItem::TurnDone(Box::new(res)),
            req = reader.read_message::<RpcRequest>() => NextRpcItem::Request(req),
        }
    } else {
        tokio::select! {
            ev = event_rx.recv() => NextRpcItem::Event(ev),
            req = reader.read_message::<RpcRequest>() => NextRpcItem::Request(req),
        }
    }
}

pub(crate) async fn handle_turn_done<W: tokio::io::AsyncWrite + Unpin>(
    ctx: &mut RpcDaemonContext<'_, W>,
    res: std::result::Result<Result<TurnOutput>, tokio::task::JoinError>,
) -> Result<()> {
    *ctx.active_turn = None;
    ctx.writer
        .write_message(&RpcEvent::StatusChanged {
            status: "idle".to_string(),
        })
        .await?;
    if let Ok(Err(err)) = res {
        ctx.writer
            .write_message(&RpcEvent::Error {
                code: "turn_error".to_string(),
                message: err.to_string(),
            })
            .await?;
    }
    let eng = ctx.engine.read().await;
    eng.force_refresh_quota();
    let totals = eng.session_usage_totals();
    let usage_ev = RpcEvent::UsageUpdate {
        input_tokens: Some(totals.total_input),
        output_tokens: Some(totals.total_output),
        cache_read_tokens: Some(totals.total_cache_read),
        cache_write_tokens: Some(totals.total_cache_write),
        total_cost: None,
        context_percent: eng.context_percent_f64(),
        context_window: eng.context_limit(),
        tokens_per_second: eng.tokens_per_second(),
        quota: eng.quota_display(),
    };
    ctx.writer.write_message(&usage_ev).await?;
    Ok(())
}

pub(crate) async fn handle_next_rpc_item<R, W>(
    reader: &mut JsonLinesReader<R>,
    event_rx: &mut mpsc::UnboundedReceiver<RpcEvent>,
    ctx: &mut RpcDaemonContext<'_, W>,
) -> Result<RpcLoopAction>
where
    R: tokio::io::AsyncBufRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    match poll_next_rpc(reader, event_rx, ctx.active_turn).await {
        NextRpcItem::Event(Some(event)) => {
            ctx.writer.write_message(&event).await?;
            Ok(RpcLoopAction::Continue)
        }
        NextRpcItem::Event(None) => Ok(RpcLoopAction::Break),
        NextRpcItem::TurnDone(res) => {
            handle_turn_done(ctx, *res).await?;
            Ok(RpcLoopAction::Continue)
        }
        NextRpcItem::Request(res) => {
            let Some(req) = handle_read_request(res, ctx.writer).await? else {
                return Ok(RpcLoopAction::Break);
            };
            dispatch_rpc((req.command, req.id), ctx).await
        }
    }
}

pub(crate) async fn run_rpc_loop<R, W>(
    reader: &mut JsonLinesReader<R>,
    event_rx: &mut mpsc::UnboundedReceiver<RpcEvent>,
    ctx: &mut RpcDaemonContext<'_, W>,
) -> Result<()>
where
    R: tokio::io::AsyncBufRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    loop {
        let action = handle_next_rpc_item(reader, event_rx, ctx).await?;
        if matches!(action, RpcLoopAction::Break) {
            break;
        }
    }
    Ok(())
}
