mod handlers;
mod loop_runner;
mod presenter;
#[cfg(test)]
mod tests;
mod types;

use std::sync::Arc;
use tokio::io::BufReader;
use tokio::sync::RwLock;

use crate::auth::AuthStore;
use crate::config::Config;
use crate::error::Result;
use crate::steering::SharedSteeringQueue;
use loop_runner::run_rpc_loop;
pub(crate) use presenter::{RpcEventQueue, RpcPresenter};
use rho_harness_core::rpc::protocol::RpcEvent;
use rho_harness_core::rpc::transport::{JsonLinesReader, JsonLinesWriter};
use types::RpcDaemonContext;

pub async fn run_rpc_session_over_stream<R, W>(
    reader_stream: R,
    writer_stream: W,
    engine_lock: Arc<RwLock<rho_engine::engine::AgentEngine>>,
    config_lock: Arc<RwLock<Config>>,
    auth_store_lock: Arc<RwLock<AuthStore>>,
) -> Result<()>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    let mut reader = JsonLinesReader::new(BufReader::new(reader_stream));
    let mut writer = JsonLinesWriter::new(writer_stream);
    let events = RpcEventQueue::default();
    let rpc_presenter = RpcPresenter::new(events.clone());
    let pending_approvals = rpc_presenter.pending_approvals();
    let presenter: Arc<dyn rho_harness_core::presentation::Presenter> = Arc::new(rpc_presenter);

    let (session_id, steering_mode) = {
        let eng = engine_lock.read().await;
        (eng.session_manager.session_id.clone(), eng.config.steering_mode)
    };
    let (model, provider) = {
        let cfg = config_lock.read().await;
        (cfg.model.clone(), cfg.provider.clone())
    };

    let steering = Arc::new(SharedSteeringQueue::new(steering_mode));

    let init = RpcEvent::SessionStart {
        session_id,
        model,
        provider,
    };
    writer.write_message(&init).await?;

    let mut active_turn = None;
    let mut ctx = RpcDaemonContext {
        writer: &mut writer,
        engine: engine_lock,
        presenter,
        config: config_lock,
        auth_store: auth_store_lock,
        pending_approvals,
        steering,
        active_turn: &mut active_turn,
        events: events.clone(),
        auth_bridge: rho_harness_core::rpc::RpcAuthBridge::new(),
        live_models: true,
    };

    let engine_for_quota = Arc::clone(&ctx.engine);
    let quota_events = events.clone();
    let mut quota_rx = ctx.engine.read().await.quota_subscribe();
    let quota_task = tokio::spawn(async move {
        while quota_rx.changed().await.is_ok() {
            let eng = engine_for_quota.read().await;
            let totals = eng.session_usage_totals();
            quota_events.push(RpcEvent::UsageUpdate {
                input_tokens: Some(totals.total_input),
                output_tokens: Some(totals.total_output),
                cache_read_tokens: Some(totals.total_cache_read),
                cache_write_tokens: Some(totals.total_cache_write),
                total_cost: None,
                context_percent: eng.context_percent_f64(),
                context_window: Some(eng.context_limit()),
                tokens_per_second: eng.tokens_per_second(),
                quota: eng.quota_display(),
            });
        }
    });

    let res = run_rpc_loop(&mut reader, &events, &mut ctx).await;
    quota_task.abort();
    events.close();
    res
}

pub async fn run_rpc_daemon(config: Config, auth_store: AuthStore) -> Result<()> {
    let catalog = rho_engine::provider::ModelCatalog::default();
    catalog.spawn_refresh_all(&config, &auth_store);
    let engine = crate::platform::agent_engine(config.clone(), auth_store.clone(), catalog, None).await?;
    crate::cli::mcp::eprint_mcp_diagnostics(&engine);
    engine.spawn_refresh_quota();
    let engine_lock = Arc::new(RwLock::new(engine));
    let config_lock = Arc::new(RwLock::new(config));
    let auth_store_lock = Arc::new(RwLock::new(auth_store));
    run_rpc_session_over_stream(
        tokio::io::stdin(),
        tokio::io::stdout(),
        engine_lock,
        config_lock,
        auth_store_lock,
    )
    .await
}
