use std::sync::Arc;

use super::super::presenter::RpcEventQueue;
use super::super::types::RpcDaemonContext;
use super::config::handle_node_info_cmd;
use super::session::{handle_create_session_cmd, write_command_result};
use crate::auth::AuthStore;
use crate::config::Config;
use crate::error::Result;
use rho_engine::provider::ModelCatalog;
use rho_harness_core::auth::StoredCredential;
use rho_harness_core::rpc::protocol::{RpcCommand, RpcEvent, RpcResponse};
use tokio::sync::RwLock;

/// Reports the login and returns whether its credential was saved.
pub(crate) async fn handle_oauth_login_result(
    provider: rho_harness_core::provider::ProviderId,
    result: Result<StoredCredential>,
    auth_store: &Arc<RwLock<AuthStore>>,
    events: &RpcEventQueue,
) -> bool {
    let saved = match result {
        Ok(cred) => {
            let mut store = auth_store.write().await;
            let login = store.login_key(provider, &cred);
            store
                .set_credential_async(&login.to_string(), cred)
                .await
                .map(|()| login)
        }
        Err(e) => Err(e),
    };
    let (login, success, error) = match saved {
        Ok(login) => (login.to_string(), true, None),
        Err(e) => (provider.as_str().to_string(), false, Some(e.to_string())),
    };
    events.push(RpcEvent::AuthComplete {
        provider: login,
        success,
        error,
    });
    success
}

/// Fetches the model list of a provider that just gained credentials.
async fn refresh_provider_models(
    catalog: &ModelCatalog,
    provider: &str,
    config: &RwLock<Config>,
    auth_store: &RwLock<AuthStore>,
) {
    catalog.spawn_refresh(provider, &*config.read().await, &*auth_store.read().await);
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
    let callbacks = ctx.auth_bridge.callbacks(&provider_str, Arc::new(ctx.events.clone()));
    let auth_store = Arc::clone(&ctx.auth_store);
    let config = Arc::clone(&ctx.config);
    let catalog = ctx.engine.read().await.model_catalog().clone();
    let events = ctx.events.clone();
    let live_models = ctx.live_models;

    tokio::spawn(async move {
        let result = rho_engine::auth::perform_oauth_login(provider_id, &callbacks).await;
        if handle_oauth_login_result(provider_id, result, &auth_store, &events).await && live_models {
            refresh_provider_models(&catalog, provider_id.as_str(), &config, &auth_store).await;
        }
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
    let saved = ctx.auth_store.write().await.set_key_async(&provider, api_key).await;
    if saved.is_ok() && ctx.live_models {
        let catalog = ctx.engine.read().await.model_catalog().clone();
        refresh_provider_models(&catalog, &provider, &ctx.config, &ctx.auth_store).await;
    }
    write_command_result(ctx.writer, req_id, "set_api_key", saved.map(|()| None)).await
}

pub(crate) async fn handle_node_session_auth_cmd<W: tokio::io::AsyncWrite + Unpin>(
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
    use rho_harness_core::error::AppError;
    use rho_test_support::TestDir;
    use test_case::test_case;

    #[test_case(Ok(StoredCredential::api_key("test-token")), false, None; "saved")]
    #[test_case(Err(AppError::Auth("login cancelled".to_string())), false, Some("login cancelled"); "login error")]
    #[test_case(Ok(StoredCredential::api_key("test-token")), true, Some("Failed to read auth file"); "save error")]
    #[tokio::test]
    async fn test_handle_oauth_login_result_outcomes(
        result: Result<StoredCredential>,
        blocked_path: bool,
        expected_error: Option<&str>,
    ) {
        let temp = TestDir::new();
        let path = temp.join("auth.json");
        let store = AuthStore::load(&path).unwrap();
        if blocked_path {
            std::fs::create_dir(&path).unwrap();
        }
        let auth_store = Arc::new(RwLock::new(store));
        let events = RpcEventQueue::default();

        let saved = handle_oauth_login_result(
            rho_harness_core::provider::ProviderId::ChatGpt,
            result,
            &auth_store,
            &events,
        )
        .await;
        assert_eq!(saved, expected_error.is_none());

        let batch = events.next_batch().await;
        let Some(
            [
                RpcEvent::AuthComplete {
                    provider,
                    success,
                    error,
                },
            ],
        ) = batch.as_deref()
        else {
            panic!("expected AuthComplete, got {batch:?}");
        };
        assert_eq!(provider, "chatgpt");
        assert_eq!(*success, expected_error.is_none());
        match expected_error {
            Some(needle) => assert!(error.as_deref().unwrap().contains(needle)),
            None => assert!(error.is_none()),
        }
    }
}
