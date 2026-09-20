mod handlers;
mod loop_runner;
#[cfg(test)]
mod tests;
mod types;

use std::sync::Arc;
use tokio::io::BufReader;
use tokio::sync::{RwLock, mpsc};

use crate::auth::AuthStore;
use crate::config::Config;
use crate::error::Result;
use crate::repl::coordinator::SharedSteeringQueue;
use crate::ui::render::RpcPresenter;
use loop_runner::run_rpc_loop;
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
    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<RpcEvent>();
    crate::platform::remote::PEER_REGISTRY.register(event_tx.clone());
    let rpc_presenter = RpcPresenter::new(event_tx.clone());
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
        event_tx,
        auth_bridge: rho_harness_core::rpc::RpcAuthBridge::new(),
    };

    let engine_for_quota = Arc::clone(&ctx.engine);
    let mut quota_rx = ctx.engine.read().await.quota_subscribe();
    let quota_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                res = quota_rx.changed() => {
                    if res.is_err() {
                        break;
                    }
                    let eng = engine_for_quota.read().await;
                    let totals = eng.session_usage_totals();
                    crate::platform::remote::PEER_REGISTRY.broadcast(&RpcEvent::UsageUpdate {
                        input_tokens: Some(totals.total_input),
                        output_tokens: Some(totals.total_output),
                        cache_read_tokens: Some(totals.total_cache_read),
                        cache_write_tokens: Some(totals.total_cache_write),
                        total_cost: None,
                        context_percent: eng.context_percent_f64(),
                        context_window: eng.context_limit(),
                        tokens_per_second: eng.tokens_per_second(),
                        quota: eng.quota_display(),
                    });
                }
                _ = interval.tick() => {
                    let eng = engine_for_quota.read().await;
                    if eng.should_refresh_quota() {
                        eng.spawn_refresh_quota();
                    }
                }
            }
        }
    });

    let res = run_rpc_loop(&mut reader, &mut event_rx, &mut ctx).await;
    quota_task.abort();
    res
}

pub async fn run_rpc_daemon(config: Config, auth_store: AuthStore) -> Result<()> {
    let engine = crate::platform::agent_engine(config.clone(), auth_store.clone(), None).await?;
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
