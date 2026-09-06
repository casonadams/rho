use super::footer::sync_turn_footer;
use super::input::reconcile_consumed_steering;
use super::runner::{TurnLoop, reset_controller_idle};
use crate::engine::runner::{CancellationSignal, TurnOutput};
use crate::error::Result;
use crate::repl::live::navigation::restore_queued_messages;
use crate::ui::interactive::{QueueKind, TerminalBackend, UiEvent};

pub(super) async fn cancel_active_turn<B: TerminalBackend>(
    lp: &mut TurnLoop<'_, B>,
    ui_events: &mut tokio::sync::mpsc::UnboundedReceiver<UiEvent>,
    cancellation: &CancellationSignal,
) -> Result<()> {
    cancellation.cancel();
    lp.steering.clear();
    reconcile_consumed_steering(lp.controller, &lp.steering);
    lp.controller
        .state_mut()
        .retain_queued(|msg| msg.kind != QueueKind::Steering);
    reset_controller_idle(lp.controller);
    lp.session.renderer.flush();
    lp.batch.drain_events(lp.controller, ui_events)?;
    reset_controller_idle(lp.controller);
    lp.engine.record_cancellation("operator interrupt").await?;
    restore_queued_messages(lp.controller);
    lp.session.renderer.print_notice("\nCanceled.\n");
    lp.batch.drain_events(lp.controller, ui_events)?;
    reset_controller_idle(lp.controller);
    lp.batch.flush(lp.controller, false)
}

pub(super) fn finish_active_turn<B: TerminalBackend>(
    lp: &mut TurnLoop<'_, B>,
    ui_events: &mut tokio::sync::mpsc::UnboundedReceiver<UiEvent>,
    result: Result<TurnOutput>,
) -> Result<()> {
    reconcile_consumed_steering(lp.controller, &lp.steering);
    lp.session.renderer.flush();
    sync_turn_footer(lp.controller, lp.engine);
    lp.batch.drain_events(lp.controller, ui_events)?;
    lp.batch.flush(lp.controller, false)?;
    if let Err(error) = result {
        reset_controller_idle(lp.controller);
        restore_queued_messages(lp.controller);
        lp.session.renderer.print_notice(&format!("\nError: {error}\n"));
        sync_turn_footer(lp.controller, lp.engine);
        lp.batch.drain_events(lp.controller, ui_events)?;
        reset_controller_idle(lp.controller);
        lp.batch.flush(lp.controller, false)?;
    }
    Ok(())
}
