use std::sync::Arc;

use super::super::types::RpcDaemonContext;
use crate::error::Result;
use rho_harness_core::presentation::types::InteractionResponse;
use rho_harness_core::rpc::protocol::{RpcEvent, RpcResponse};

pub(crate) async fn handle_compact_cmd(
    engine: &rho_engine::engine::AgentEngine,
    instructions: Option<String>,
    req_id: Option<String>,
) -> RpcResponse {
    match engine.compact_session(instructions.as_deref()).await {
        Ok(s) => RpcResponse::success(
            req_id,
            "compact",
            Some(serde_json::json!({
                "tokens_before": s.tokens_before,
                "tokens_after": s.tokens_after,
                "saved_tokens": s.saved_tokens,
            })),
        ),
        Err(e) => RpcResponse::failure(req_id, "compact", &e.to_string()),
    }
}

pub(crate) async fn handle_steer_command<W: tokio::io::AsyncWrite + Unpin>(
    (message, req_id): (String, Option<String>),
    ctx: &mut RpcDaemonContext<'_, W>,
) -> Result<()> {
    if let Some(steering) = crate::platform::remote::get_active_steering() {
        steering.enqueue(message.clone());
        crate::platform::remote::PEER_REGISTRY.broadcast(&rho_harness_core::rpc::protocol::RpcEvent::TextChunk {
            content: format!("\n[Steering from remote]: {message}\n"),
        });
    }
    ctx.steering.enqueue(message);
    ctx.writer
        .write_message(&RpcResponse::success(req_id, "steer", None))
        .await
}

pub(crate) async fn handle_abort_cmd<W: tokio::io::AsyncWrite + Unpin>(
    req_id: Option<String>,
    ctx: &mut RpcDaemonContext<'_, W>,
) -> Result<()> {
    if let Some(handle) = ctx.active_turn.take() {
        handle.abort();
    }
    ctx.steering.clear();
    let eng = ctx.engine.read().await;
    let _ = eng.record_cancellation("rpc abort").await;
    ctx.writer
        .write_message(&RpcEvent::StatusChanged {
            status: "idle".to_string(),
        })
        .await?;
    ctx.writer
        .write_message(&RpcResponse::success(req_id, "abort", None))
        .await
}

pub(crate) fn parse_tool_decision(decision: &str) -> InteractionResponse {
    let lower = decision.trim().to_lowercase();
    match lower.as_str() {
        "allow" | "0" => InteractionResponse::Selected(0),
        "always" | "always_allow" | "2" => InteractionResponse::Selected(2),
        "deny" | "denied" | "cancel" | "cancelled" => InteractionResponse::Cancelled,
        d if d.starts_with("edit:") => InteractionResponse::SelectedWithInput {
            index: 1,
            text: decision.trim()[5..].trim().to_string(),
        },
        d if d.starts_with("deny:") => InteractionResponse::SelectedWithInput {
            index: 3,
            text: decision.trim()[5..].trim().to_string(),
        },
        _ => InteractionResponse::Selected(0),
    }
}

pub(crate) async fn handle_tool_response_cmd<W: tokio::io::AsyncWrite + Unpin>(
    (approval_id, decision, req_id): (String, String, Option<String>),
    ctx: &mut RpcDaemonContext<'_, W>,
) -> Result<()> {
    let resp = parse_tool_decision(&decision);
    if let Some(sender) = crate::platform::remote::ACTIVE_APPROVALS
        .lock()
        .unwrap()
        .remove(&approval_id)
    {
        let _ = sender.send(resp.clone());
    }
    let mut approvals = ctx.pending_approvals.lock().await;
    if let Some(sender) = approvals.remove(&approval_id) {
        let _ = sender.send(resp);
    }
    ctx.writer
        .write_message(&RpcResponse::success(req_id, "tool_response", None))
        .await
}

pub(crate) async fn handle_prompt_cmd<W: tokio::io::AsyncWrite + Unpin>(
    (message, req_id): (String, Option<String>),
    ctx: &mut RpcDaemonContext<'_, W>,
) -> Result<()> {
    if crate::platform::remote::is_repl_active() {
        if let Some(steering) = crate::platform::remote::get_active_steering() {
            steering.enqueue(message.clone());
            crate::platform::remote::PEER_REGISTRY.broadcast(&rho_harness_core::rpc::protocol::RpcEvent::TextChunk {
                content: format!("\n[Steering from remote]: {message}\n"),
            });
            ctx.writer
                .write_message(&RpcResponse::success(
                    req_id,
                    "prompt",
                    Some(serde_json::json!({ "steered": true })),
                ))
                .await?;
            return Ok(());
        }

        crate::platform::remote::REMOTE_PROMPT_QUEUE.push(message);
        ctx.writer
            .write_message(&RpcResponse::success(req_id, "prompt", None))
            .await?;
        return Ok(());
    }

    if ctx.active_turn.is_some() {
        ctx.writer
            .write_message(&RpcResponse::failure(
                req_id,
                "prompt",
                "A turn is already in progress. Steer or abort the active turn first.",
            ))
            .await?;
        return Ok(());
    }

    ctx.writer
        .write_message(&RpcResponse::success(req_id, "prompt", None))
        .await?;
    ctx.writer
        .write_message(&RpcEvent::StatusChanged {
            status: "busy".to_string(),
        })
        .await?;

    let engine = Arc::clone(&ctx.engine);
    let presenter = Arc::clone(&ctx.presenter);
    let steering = Arc::clone(&ctx.steering);
    let handle = tokio::spawn(async move {
        let req = crate::engine::runner::TurnRequest::new(&message).with_steering(steering);
        let eng = engine.read().await;
        eng.run_turn(req, presenter).await
    });
    *ctx.active_turn = Some(handle);
    Ok(())
}
