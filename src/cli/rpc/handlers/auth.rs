use std::sync::Arc;

use super::super::types::RpcDaemonContext;
use super::config::handle_node_info_cmd;
use super::session::handle_create_session_cmd;
use crate::error::Result;
use rho_harness_core::auth::StoredCredential;
use rho_harness_core::rpc::protocol::{RpcCommand, RpcEvent, RpcResponse};

pub(crate) async fn handle_oauth_login_result(
    provider: String,
    result: Result<StoredCredential>,
    auth_store: &Arc<tokio::sync::RwLock<crate::auth::AuthStore>>,
    event_tx: &tokio::sync::mpsc::UnboundedSender<RpcEvent>,
) {
    match result {
        Ok(cred) => {
            let mut store = auth_store.write().await;
            let _ = store.set_credential_async(&provider, cred).await;
            let _ = store.save_async().await;
            let _ = event_tx.send(RpcEvent::AuthComplete {
                provider,
                success: true,
                error: None,
            });
        }
        Err(e) => {
            let _ = event_tx.send(RpcEvent::AuthComplete {
                provider,
                success: false,
                error: Some(e.to_string()),
            });
        }
    }
}

pub(crate) async fn handle_auth_login_cmd<W: tokio::io::AsyncWrite + Unpin>(
    provider_str: String,
    req_id: Option<String>,
    ctx: &mut RpcDaemonContext<'_, W>,
) -> Result<()> {
    let Ok(provider_id) = provider_str.parse::<rho_harness_core::provider::ProviderId>() else {
        ctx.writer
            .write_message(&RpcResponse::failure(
                req_id,
                "auth_login",
                &format!("Unknown provider '{provider_str}'"),
            ))
            .await?;
        return Ok(());
    };
    let callbacks = ctx.auth_bridge.callbacks(&provider_str, ctx.event_tx.clone());
    let auth_store = Arc::clone(&ctx.auth_store);
    let event_tx = ctx.event_tx.clone();
    let prov = provider_str.clone();

    tokio::spawn(async move {
        let result = rho_engine::auth::perform_oauth_login(provider_id, &callbacks).await;
        handle_oauth_login_result(prov, result, &auth_store, &event_tx).await;
    });

    ctx.writer
        .write_message(&RpcResponse::success(req_id, "auth_login", None))
        .await?;
    Ok(())
}

pub(crate) async fn handle_auth_input_cmd<W: tokio::io::AsyncWrite + Unpin>(
    interaction_id: String,
    secret_value: Option<String>,
    selected_option: Option<String>,
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
    provider: String,
    api_key: String,
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
                interaction_id.clone(),
                secret_value.clone(),
                selected_option.clone(),
                req_id,
                ctx,
            )
            .await?;
            Ok(true)
        }
        RpcCommand::SetApiKey { provider, api_key } => {
            handle_set_api_key_cmd(provider.clone(), api_key.clone(), req_id, ctx).await?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::AuthStore;
    use rho_harness_core::auth::StoredCredential;
    use rho_harness_core::error::AppError;
    use tempfile::tempdir;
    use tokio::sync::{RwLock, mpsc};

    #[tokio::test]
    async fn test_handle_oauth_login_result_success() {
        let temp = tempdir().unwrap();
        let auth_store = Arc::new(RwLock::new(AuthStore::load(temp.path().join("auth.json")).unwrap()));
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();

        let cred = StoredCredential::api_key("test-token");
        handle_oauth_login_result("test_prov".to_string(), Ok(cred), &auth_store, &event_tx).await;

        let event = event_rx.recv().await.unwrap();
        assert_eq!(
            event,
            RpcEvent::AuthComplete {
                provider: "test_prov".to_string(),
                success: true,
                error: None,
            }
        );
        let store = auth_store.read().await;
        assert!(store.get_credential("test_prov").is_some());
    }

    #[tokio::test]
    async fn test_handle_oauth_login_result_failure() {
        let temp = tempdir().unwrap();
        let auth_store = Arc::new(RwLock::new(AuthStore::load(temp.path().join("auth.json")).unwrap()));
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();

        let err = Err(AppError::Auth("login cancelled".to_string()));
        handle_oauth_login_result("test_prov".to_string(), err, &auth_store, &event_tx).await;

        let event = event_rx.recv().await.unwrap();
        assert_eq!(
            event,
            RpcEvent::AuthComplete {
                provider: "test_prov".to_string(),
                success: false,
                error: Some("Auth error: login cancelled".to_string()),
            }
        );
    }
}
