use std::sync::Arc;

use super::super::types::RpcDaemonContext;
use super::config::handle_node_info_cmd;
use super::session::handle_create_session_cmd;
use crate::error::Result;
use rho_harness_core::rpc::protocol::{RpcCommand, RpcEvent, RpcResponse};

pub(crate) async fn handle_auth_login_cmd<W: tokio::io::AsyncWrite + Unpin>(
    provider_str: String,
    req_id: Option<String>,
    ctx: &mut RpcDaemonContext<'_, W>,
) -> Result<()> {
    let provider_id = match provider_str.parse::<rho_harness_core::provider::ProviderId>() {
        Ok(p) => p,
        Err(_) => {
            ctx.writer
                .write_message(&RpcResponse::failure(
                    req_id,
                    "auth_login",
                    &format!("Unknown provider '{provider_str}'"),
                ))
                .await?;
            return Ok(());
        }
    };
    let callbacks = ctx.auth_bridge.callbacks(&provider_str, ctx.event_tx.clone());
    let auth_store = Arc::clone(&ctx.auth_store);
    let event_tx = ctx.event_tx.clone();
    let prov = provider_str.clone();

    tokio::spawn(async move {
        match rho_engine::auth::perform_oauth_login(provider_id, &callbacks).await {
            Ok(cred) => {
                let mut store = auth_store.write().await;
                let _ = store.set_credential_async(&prov, cred).await;
                let _ = store.save_async().await;
                let _ = event_tx.send(RpcEvent::AuthComplete {
                    provider: prov,
                    success: true,
                    error: None,
                });
            }
            Err(e) => {
                let _ = event_tx.send(RpcEvent::AuthComplete {
                    provider: prov,
                    success: false,
                    error: Some(e.to_string()),
                });
            }
        }
    });

    ctx.writer
        .write_message(&RpcResponse::success(req_id, "auth_login", None))
        .await?;
    Ok(())
}

pub(crate) async fn handle_auth_input_cmd<W: tokio::io::AsyncWrite + Unpin>(
    (interaction_id, secret_value, selected_option): (String, Option<String>, Option<String>),
    req_id: Option<String>,
    ctx: &mut RpcDaemonContext<'_, W>,
) -> Result<()> {
    let resolved = ctx.auth_bridge.resolve_input(
        &interaction_id,
        rho_harness_core::rpc::AuthInputResponse {
            secret_value,
            selected_option,
        },
    );
    let payload = serde_json::json!({ "resolved": resolved });
    ctx.writer
        .write_message(&RpcResponse::success(req_id, "auth_input", Some(payload)))
        .await?;
    Ok(())
}

pub(crate) async fn handle_set_api_key_cmd<W: tokio::io::AsyncWrite + Unpin>(
    (provider, api_key): (String, String),
    req_id: Option<String>,
    ctx: &mut RpcDaemonContext<'_, W>,
) -> Result<()> {
    let mut store = ctx.auth_store.write().await;
    match store.set_api_key_async(&provider, api_key).await {
        Ok(_) => {
            let _ = store.save_async().await;
            ctx.writer
                .write_message(&RpcResponse::success(req_id, "set_api_key", None))
                .await?;
        }
        Err(e) => {
            ctx.writer
                .write_message(&RpcResponse::failure(req_id, "set_api_key", &e.to_string()))
                .await?;
        }
    }
    Ok(())
}

pub(crate) async fn handle_remote_auth_cmd<W: tokio::io::AsyncWrite + Unpin>(
    cmd: &RpcCommand,
    req_id: Option<String>,
    ctx: &mut RpcDaemonContext<'_, W>,
) -> Result<bool> {
    match cmd {
        RpcCommand::GetNodeInfo => {
            handle_node_info_cmd(req_id, ctx).await?;
            Ok(true)
        }
        RpcCommand::CreateSession { workspace } => {
            handle_create_session_cmd(workspace.clone(), req_id, ctx).await?;
            Ok(true)
        }
        RpcCommand::AuthLogin { provider } => {
            handle_auth_login_cmd(provider.clone(), req_id, ctx).await?;
            Ok(true)
        }
        RpcCommand::AuthInput {
            interaction_id,
            secret_value,
            selected_option,
        } => {
            handle_auth_input_cmd(
                (interaction_id.clone(), secret_value.clone(), selected_option.clone()),
                req_id,
                ctx,
            )
            .await?;
            Ok(true)
        }
        RpcCommand::SetApiKey { provider, api_key } => {
            handle_set_api_key_cmd((provider.clone(), api_key.clone()), req_id, ctx).await?;
            Ok(true)
        }
        _ => Ok(false),
    }
}
