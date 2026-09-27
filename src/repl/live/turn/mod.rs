mod cancel;
pub(crate) mod collab;
mod event;
pub(crate) mod footer;
mod input;
mod model_switch;
mod runner;
#[cfg(test)]
mod tests;

use std::sync::Arc;

#[cfg(test)]
pub(crate) use super::batch::LiveBatch;
pub(crate) use footer::sync_turn_footer;
#[cfg(test)]
pub(crate) use model_switch::{TurnModelSwitchInput, apply_turn_model_switch};

use super::ActiveTurn;
use super::batch::OUTPUT_FRAME_INTERVAL;
use super::types::TerminalInputReader;
use crate::engine::AgentEngine;
use crate::engine::runner::{CancellationSignal, TurnOutput, TurnRequest};
use crate::error::Result;
use crate::repl::coordinator::SharedSteeringQueue;
use crate::ui::interactive::TerminalBackend;

use cancel::finish_active_turn;
use event::{TurnInputResources, dispatch_turn_input};
use runner::TurnLoop;

struct TurnContext<'a, B: TerminalBackend> {
    loop_ctx: TurnLoop<'a, B>,
    input_reader: &'a mut TerminalInputReader,
    resources: TurnInputResources<'a>,
}

const WORKING_QUOTA_CHECK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30);

enum TurnEvent {
    Tick,
    QuotaPeriodic,
    QuotaUpdated,
    Input(Option<std::io::Result<crossterm::event::Event>>),
    Ui(crate::ui::interactive::UiEvent),
    CollabCommand(rho_harness_core::collab::CollabIncomingCommand),
    CollabPeer(rho_harness_core::collab::CollabPeerEvent),
}

fn build_turn_context<'a, B: TerminalBackend>(
    session: &'a mut crate::repl::ReplSession,
    engine: &'a AgentEngine,
    turn: &'a mut ActiveTurn<'_, B>,
    cancellation: &'a Arc<CancellationSignal>,
) -> (TurnContext<'a, B>, TurnRequest<'a>) {
    let steering = Arc::new(SharedSteeringQueue::new(engine.config.steering_mode));
    let model_switch = Arc::new(rho_engine::engine::runner::SharedModelSwitch::new());
    let prompt = std::mem::take(&mut turn.prompt);
    let request = TurnRequest::new(prompt)
        .with_cancellation(cancellation)
        .with_steering(steering.clone())
        .with_model_switch(model_switch.clone());
    let loop_ctx = TurnLoop::new(session, engine, turn.io.controller, steering, model_switch);
    let resources = TurnInputResources {
        history: turn.editor.history,
        completions: turn.editor.completions,
        ui_events: turn.io.events,
        cancellation,
    };
    (
        TurnContext {
            loop_ctx,
            input_reader: turn.io.input,
            resources,
        },
        request,
    )
}

async fn handle_input_res<B: TerminalBackend>(
    ctx: &mut TurnContext<'_, B>,
    res: Option<std::io::Result<crossterm::event::Event>>,
) -> Result<bool> {
    let Some(event_res) = res else {
        ctx.loop_ctx.batch.flush(ctx.loop_ctx.controller, false)?;
        return Err(anyhow::anyhow!("Terminal input reader stopped").into());
    };
    let event = match event_res {
        Ok(e) => e,
        Err(err) => {
            ctx.loop_ctx.batch.flush(ctx.loop_ctx.controller, false)?;
            return Err(err.into());
        }
    };
    dispatch_turn_input(&mut ctx.loop_ctx, &mut ctx.resources, event).await
}

async fn wait_turn_quota(rx: &mut tokio::sync::watch::Receiver<u64>) {
    if rx.changed().await.is_err() {
        std::future::pending::<()>().await;
    }
}

async fn wait_collab_cmd(
    collab: Option<&Arc<rho_harness_core::collab::CollabHostServer>>,
) -> Option<rho_harness_core::collab::CollabIncomingCommand> {
    match collab {
        Some(c) => c.recv_command().await,
        None => std::future::pending().await,
    }
}

async fn wait_collab_peer(
    collab: Option<&Arc<rho_harness_core::collab::CollabHostServer>>,
) -> Option<rho_harness_core::collab::CollabPeerEvent> {
    match collab {
        Some(c) => c.recv_peer_event().await,
        None => std::future::pending().await,
    }
}

async fn next_turn_event(
    frame: &mut tokio::time::Interval,
    periodic_quota: &mut tokio::time::Interval,
    quota_rx: &mut tokio::sync::watch::Receiver<u64>,
    input: &mut TerminalInputReader,
    ui: &mut tokio::sync::mpsc::UnboundedReceiver<crate::ui::interactive::UiEvent>,
    collab: Option<&Arc<rho_harness_core::collab::CollabHostServer>>,
) -> TurnEvent {
    tokio::select! {
        biased;
        res = input.recv() => TurnEvent::Input(res),
        Some(cmd) = wait_collab_cmd(collab) => TurnEvent::CollabCommand(cmd),
        Some(pe) = wait_collab_peer(collab) => TurnEvent::CollabPeer(pe),
        _ = wait_turn_quota(quota_rx) => TurnEvent::QuotaUpdated,
        _ = periodic_quota.tick() => TurnEvent::QuotaPeriodic,
        _ = frame.tick() => TurnEvent::Tick,
        Some(ev) = ui.recv() => TurnEvent::Ui(ev),
    }
}

async fn handle_turn_collab_event<B: TerminalBackend>(
    ctx: &mut TurnContext<'_, B>,
    collab_presenter: Option<&collab::CollabPresenter>,
    ev: TurnEvent,
) -> Result<bool> {
    match ev {
        TurnEvent::CollabCommand(incoming) => {
            collab::handle_collab_turn_command(&mut ctx.loop_ctx, &mut ctx.resources, collab_presenter, incoming).await
        }
        TurnEvent::CollabPeer(event) => {
            let count = ctx
                .loop_ctx
                .session
                .collab
                .as_ref()
                .map(|c| c.peer_count_sync())
                .unwrap_or(0);
            collab::handle_collab_peer_event(&event, &ctx.loop_ctx.session.renderer, count);
            let f = ctx.loop_ctx.controller.state_mut().footer_mut();
            f.remote_active = true;
            f.remote_peers = count;
            f.extra_status = Some(format!("[Collab: {count}]"));
            ctx.loop_ctx.batch.flush(ctx.loop_ctx.controller, true)?;
            Ok(false)
        }
        _ => Ok(false),
    }
}

async fn handle_turn_event<B: TerminalBackend>(
    ctx: &mut TurnContext<'_, B>,
    collab_presenter: Option<&collab::CollabPresenter>,
    ev: TurnEvent,
) -> Result<bool> {
    match ev {
        TurnEvent::Tick => {
            let prev_w = ctx.loop_ctx.controller.width();
            ctx.loop_ctx.on_tick()?;
            if ctx.loop_ctx.controller.width() != prev_w {
                ctx.loop_ctx.session.renderer.set_width(ctx.loop_ctx.controller.width());
            }
            Ok(false)
        }
        TurnEvent::QuotaPeriodic => {
            if ctx.loop_ctx.engine.should_refresh_quota() {
                ctx.loop_ctx.engine.spawn_refresh_quota();
            }
            let changed = sync_turn_footer(ctx.loop_ctx.controller, ctx.loop_ctx.engine);
            if changed {
                ctx.loop_ctx.batch.flush(ctx.loop_ctx.controller, true)?;
            }
            Ok(false)
        }
        TurnEvent::QuotaUpdated => {
            let changed = sync_turn_footer(ctx.loop_ctx.controller, ctx.loop_ctx.engine);
            if changed {
                ctx.loop_ctx.batch.flush(ctx.loop_ctx.controller, true)?;
            }
            Ok(false)
        }
        TurnEvent::Input(res) => handle_input_res(ctx, res).await,
        TurnEvent::Ui(ev) => {
            ctx.loop_ctx.handle_ui_event(ctx.resources.ui_events, ev)?;
            Ok(false)
        }
        TurnEvent::CollabCommand(_) | TurnEvent::CollabPeer(_) => {
            handle_turn_collab_event(ctx, collab_presenter, ev).await
        }
    }
}

use futures::future::{Either, select};

async fn step_turn_select<B: TerminalBackend>(
    ctx: &mut TurnContext<'_, B>,
    collab_presenter: Option<&collab::CollabPresenter>,
    run: &mut (dyn std::future::Future<Output = Result<TurnOutput>> + Send + std::marker::Unpin),
    frame: &mut tokio::time::Interval,
    periodic_quota: &mut tokio::time::Interval,
    quota_rx: &mut tokio::sync::watch::Receiver<u64>,
) -> Result<bool> {
    let outcome = {
        let collab = ctx.loop_ctx.session.collab.as_ref();
        let ev_fut = std::pin::pin!(next_turn_event(
            frame,
            periodic_quota,
            quota_rx,
            ctx.input_reader,
            ctx.resources.ui_events,
            collab,
        ));
        match select(run, ev_fut).await {
            Either::Left((res, _)) => Either::Left(res),
            Either::Right((ev, _)) => Either::Right(ev),
        }
    };
    match outcome {
        Either::Left(res) => finish_active_turn(&mut ctx.loop_ctx, ctx.resources.ui_events, res).map(|_| true),
        Either::Right(ev) => handle_turn_event(ctx, collab_presenter, ev).await,
    }
}

pub(crate) async fn run_active_turn<B: crate::ui::interactive::TerminalBackend>(
    session: &mut crate::repl::ReplSession,
    engine: &AgentEngine,
    mut turn: ActiveTurn<'_, B>,
) -> Result<()> {
    let (presenter, collab_presenter) = if let Some(ref collab) = session.collab {
        let cp = Arc::new(collab::CollabPresenter::new(
            Arc::new(session.renderer.clone()),
            Arc::clone(collab),
        ));
        (
            Arc::clone(&cp) as Arc<dyn rho_harness_core::presentation::Presenter>,
            Some(cp),
        )
    } else {
        (
            Arc::new(session.renderer.clone()) as Arc<dyn rho_harness_core::presentation::Presenter>,
            None,
        )
    };
    let cancellation = Arc::new(CancellationSignal::default());
    let (mut ctx, request) = build_turn_context(session, engine, &mut turn, &cancellation);
    ctx.loop_ctx.batch.flush(ctx.loop_ctx.controller, true)?;
    let mut run = Box::pin(engine.run_turn(request, presenter));
    let mut frame = tokio::time::interval(OUTPUT_FRAME_INTERVAL);
    frame.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut periodic_quota = tokio::time::interval_at(
        tokio::time::Instant::now() + WORKING_QUOTA_CHECK_INTERVAL,
        WORKING_QUOTA_CHECK_INTERVAL,
    );
    periodic_quota.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut quota_rx = engine.quota_subscribe();
    while !step_turn_select(
        &mut ctx,
        collab_presenter.as_deref(),
        &mut run,
        &mut frame,
        &mut periodic_quota,
        &mut quota_rx,
    )
    .await?
    {}

    if let Some(ref cp) = collab_presenter
        && let Ok(tree) = engine.session_manager.load_tree().await
    {
        let msgs = tree.active_messages();
        let turns = rho_harness_core::session::turns::extract_turns(&msgs);
        let turns_val: Vec<serde_json::Value> =
            turns.into_iter().filter_map(|t| serde_json::to_value(t).ok()).collect();
        cp.collab
            .update_snapshot(rho_harness_core::collab::CollabSnapshot::new(turns_val, "idle"))
            .await;
    }
    Ok(())
}
