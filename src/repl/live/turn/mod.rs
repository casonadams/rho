mod cancel;
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

enum TurnEvent {
    Tick,
    Input(Option<std::io::Result<crossterm::event::Event>>),
    Ui(Option<crate::ui::interactive::UiEvent>),
}

fn build_turn_context<'a, B: TerminalBackend>(
    (session, engine): (&'a mut crate::repl::ReplSession, &'a AgentEngine),
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
    let loop_ctx = TurnLoop::new((session, engine), turn.io.controller, (steering, model_switch));
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
    let Some(event_res) = res else { return Ok(false) };
    let event = match event_res {
        Ok(e) => e,
        Err(err) => {
            ctx.loop_ctx.batch.flush(ctx.loop_ctx.controller, false)?;
            return Err(err.into());
        }
    };
    dispatch_turn_input(&mut ctx.loop_ctx, &mut ctx.resources, event).await
}

async fn wait_io(
    input: &mut TerminalInputReader,
    ui: &mut tokio::sync::mpsc::UnboundedReceiver<crate::ui::interactive::UiEvent>,
) -> TurnEvent {
    tokio::select! {
        res = input.recv() => TurnEvent::Input(res),
        ev = ui.recv() => TurnEvent::Ui(ev),
    }
}

async fn next_turn_event(
    frame: &mut tokio::time::Interval,
    input: &mut TerminalInputReader,
    ui: &mut tokio::sync::mpsc::UnboundedReceiver<crate::ui::interactive::UiEvent>,
) -> TurnEvent {
    tokio::select! {
        _ = frame.tick() => TurnEvent::Tick,
        ev = wait_io(input, ui) => ev,
    }
}

async fn handle_turn_event<B: TerminalBackend>(ctx: &mut TurnContext<'_, B>, ev: TurnEvent) -> Result<bool> {
    match ev {
        TurnEvent::Tick => {
            ctx.loop_ctx.on_tick()?;
            Ok(false)
        }
        TurnEvent::Input(res) => handle_input_res(ctx, res).await,
        TurnEvent::Ui(Some(ev)) => {
            ctx.loop_ctx.batch.push_event(ctx.loop_ctx.controller, ev)?;
            ctx.loop_ctx.drain_ui_batch(ctx.resources.ui_events)?;
            Ok(false)
        }
        TurnEvent::Ui(None) => Ok(false),
    }
}

use futures::future::{Either, select};

async fn step_turn_select<B: TerminalBackend>(
    ctx: &mut TurnContext<'_, B>,
    run: &mut (dyn std::future::Future<Output = Result<TurnOutput>> + Send + std::marker::Unpin),
    frame: &mut tokio::time::Interval,
) -> Result<bool> {
    let outcome = {
        let ev_fut = std::pin::pin!(next_turn_event(frame, ctx.input_reader, ctx.resources.ui_events));
        match select(run, ev_fut).await {
            Either::Left((res, _)) => Either::Left(res),
            Either::Right((ev, _)) => Either::Right(ev),
        }
    };
    match outcome {
        Either::Left(res) => finish_active_turn(&mut ctx.loop_ctx, ctx.resources.ui_events, res).map(|_| true),
        Either::Right(ev) => handle_turn_event(ctx, ev).await,
    }
}

pub(crate) async fn run_active_turn<B: crate::ui::interactive::TerminalBackend>(
    session: &mut crate::repl::ReplSession,
    engine: &AgentEngine,
    mut turn: ActiveTurn<'_, B>,
) -> Result<()> {
    let renderer = std::sync::Arc::new(session.renderer.clone());
    let cancellation = Arc::new(CancellationSignal::default());
    let (mut ctx, request) = build_turn_context((session, engine), &mut turn, &cancellation);
    let mut run = Box::pin(engine.run_turn(request, renderer));
    let mut frame = tokio::time::interval(OUTPUT_FRAME_INTERVAL);
    frame.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    while !step_turn_select(&mut ctx, &mut run, &mut frame).await? {}
    Ok(())
}
